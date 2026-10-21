// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/dxf.ts

//! DXF import: each shape in model space becomes one feature definition
//! with one instance, named after its layer (`docs/specs/phase-2/design.md`,
//! "Algorithms → DXF import (dc-import::dxf)").
//!
//! Parsing (group codes, sections, blocks, entity structs) is delegated to
//! the `dxf` crate. The `$INSUNITS` table, the entity set, bulge arcs, the
//! INSERT transform and its skip rules, stitching with closing, and the
//! dedupe follow PureCutCNC. Changed from upstream: DXF is already Y-up, so
//! there is no Y flip and no arc-direction flip (ADR-0001); `ELLIPSE` is
//! read; the join tolerance defaults to 0.01 mm instead of 0.5 mm; every
//! skipped entity is counted in a warning instead of dropped silently; and
//! arcs stay arcs (bulges) instead of being sampled.

use crate::{ImportError, ImportWarning};
use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, Rect};
use dc_project::{FeatureDefinition, FeatureInstance, Units};

use crate::svg::{MAX_IMPORT_VERTICES, MIN_FLATTEN_TOLERANCE_MM};

/// Default distance within which the ends of two open segments on one
/// layer are joined, mm. CAD programs write touching ends exactly; this
/// covers their float rounding and nothing a cutter could resolve.
pub const JOIN_TOLERANCE_DEFAULT_MM: f64 = 0.01;

/// How many levels of INSERT within INSERT are expanded. Deeper nesting is
/// skipped with [`ImportWarning::DxfInsertTooDeep`]; it is almost always a
/// block that inserts itself.
pub const MAX_INSERT_DEPTH: usize = 12;

/// The drawing units a DXF declares in `$INSUNITS`, among those DreamCarve
/// scales from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DxfUnits {
    /// `$INSUNITS` 1.
    Inches,
    /// `$INSUNITS` 2.
    Feet,
    /// `$INSUNITS` 3.
    Miles,
    /// `$INSUNITS` 4.
    Millimetres,
    /// `$INSUNITS` 5.
    Centimetres,
    /// `$INSUNITS` 6.
    Metres,
}

impl DxfUnits {
    /// Millimetres per drawing unit.
    #[must_use]
    pub const fn mm_per_unit(self) -> f64 {
        match self {
            Self::Inches => 25.4,
            Self::Feet => 25.4 * 12.0,
            Self::Miles => 25.4 * 12.0 * 5280.0,
            Self::Millimetres => 1.0,
            Self::Centimetres => 10.0,
            Self::Metres => 1000.0,
        }
    }
}

impl std::fmt::Display for DxfUnits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Inches => "inch",
            Self::Feet => "foot",
            Self::Miles => "mile",
            Self::Millimetres => "mm",
            Self::Centimetres => "cm",
            Self::Metres => "m",
        })
    }
}

/// Parameters of a DXF import.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfImportOptions {
    /// The drawing's units, overriding `$INSUNITS`. Required when the file
    /// declares none; `None` reads the file's.
    pub units: Option<Units>,
    /// Open segments on one layer whose ends lie within this distance are
    /// joined, and a chain whose ends meet within it is closed, mm. Zero
    /// joins only ends that coincide.
    pub join_tolerance_mm: f64,
    /// Only shapes on these layers are imported; `None` imports every
    /// layer. Layer `0` matches `"0"`.
    pub layers: Option<Vec<String>>,
    /// Furthest a flattened SPLINE or ELLIPSE may stray from the curve, mm.
    /// At least [`MIN_FLATTEN_TOLERANCE_MM`].
    pub flatten_tolerance_mm: f64,
    /// Most vertices the whole import may produce before it fails with
    /// [`ImportError::TooManyVertices`].
    pub max_vertices: usize,
}

impl DxfImportOptions {
    /// Options that read the file's units, join within
    /// [`JOIN_TOLERANCE_DEFAULT_MM`], import every layer, and flatten at
    /// [`FLATTEN_TOLERANCE_DEFAULT_MM`] under [`MAX_IMPORT_VERTICES`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            units: None,
            join_tolerance_mm: JOIN_TOLERANCE_DEFAULT_MM,
            layers: None,
            flatten_tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
            max_vertices: MAX_IMPORT_VERTICES,
        }
    }
}

impl Default for DxfImportOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// The result of a DXF import, in millimetres, Y-up, with the DXF origin
/// as the origin.
#[derive(Debug, Clone, PartialEq)]
pub struct DxfImport {
    /// One definition per shape, in document order of the shape's first
    /// entity. Ids are `d1`, `d2`, ...; names are the layer name, or
    /// `<layer>-N` (N from 1, in document order) when a layer holds several
    /// shapes.
    pub definitions: Vec<FeatureDefinition>,
    /// One instance per definition, same order. Ids are `i1`, `i2`, ...;
    /// identity transform, `z_top_mm` and `z_bottom_mm` unset (`None`).
    pub instances: Vec<FeatureInstance>,
    /// Bounds of all imported polylines, mm; `None` when nothing imported.
    pub bounds: Option<Rect>,
    /// The units `$INSUNITS` declared, when DreamCarve knows them; `None`
    /// when absent or unsupported (then `options.units` was used).
    pub units: Option<DxfUnits>,
    /// Millimetres per drawing unit, as applied.
    pub mm_per_unit: f64,
}

/// Imports the DXF document in `bytes`.
///
/// Reads `LINE`, `CIRCLE`, `ARC`, `LWPOLYLINE`, `POLYLINE` (with bulges,
/// kept as arcs), `SPLINE` and `ELLIPSE` (flattened to
/// `options.flatten_tolerance_mm`), and `INSERT` (expanded with its
/// transform, up to [`MAX_INSERT_DEPTH`] deep) from model space. Shapes are
/// scaled to millimetres by `$INSUNITS` or `options.units`. Open segments
/// on one layer are stitched within `options.join_tolerance_mm`, identical
/// shapes on one layer deduped, and every other entity type counted in
/// [`ImportWarning::DxfEntitiesSkipped`].
///
/// # Errors
///
/// [`ImportError`] when the bytes are not a readable DXF, the units are
/// unknown and not overridden, a coordinate is not finite or beyond
/// [`dc_geometry::MAX_COORDINATE_MM`], or an option is out of range.
pub fn import_dxf(
    bytes: &[u8],
    options: &DxfImportOptions,
) -> Result<(DxfImport, Vec<ImportWarning>), ImportError> {
    let tolerance_mm = options.flatten_tolerance_mm;
    if !(tolerance_mm.is_finite() && tolerance_mm >= MIN_FLATTEN_TOLERANCE_MM) {
        return Err(ImportError::InvalidTolerance { tolerance_mm });
    }
    let join_mm = options.join_tolerance_mm;
    if !(join_mm.is_finite() && join_mm >= 0.0) {
        return Err(ImportError::InvalidJoinTolerance {
            tolerance_mm: join_mm,
        });
    }
    let _ = bytes;
    Err(ImportError::Unimplemented)
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
    use crate::{SvgImportOptions, import_svg};
    use dc_geometry::{
        MAX_COORDINATE_MM, Point, Polyline, Segment, Transform, Vec2, assert_approx_eq,
    };
    use dc_project::{DefinitionId, InstanceId};
    use std::collections::BTreeMap;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    /// Coordinates in the fixtures are short decimals; this covers the
    /// rounding of a transform or a unit conversion.
    const TOL_MM: f64 = 1e-6;

    macro_rules! fixture {
        ($name:literal) => {
            include_bytes!(concat!("../../../tests/fixtures/dxf/", $name))
        };
    }

    fn import(bytes: &[u8]) -> (DxfImport, Vec<ImportWarning>) {
        import_dxf(bytes, &DxfImportOptions::new()).expect("import succeeds")
    }

    fn import_with(bytes: &[u8], options: DxfImportOptions) -> (DxfImport, Vec<ImportWarning>) {
        import_dxf(bytes, &options).expect("import succeeds")
    }

    fn names(result: &DxfImport) -> Vec<&str> {
        result.definitions.iter().map(|d| d.name.as_str()).collect()
    }

    fn def<'a>(result: &'a DxfImport, name: &str) -> &'a FeatureDefinition {
        result
            .definitions
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("no definition named {name}; have {:?}", names(result)))
    }

    /// The one profile of the definition named `name`.
    fn only<'a>(result: &'a DxfImport, name: &str) -> &'a Polyline {
        let profiles = &def(result, name).profiles;
        assert_eq!(profiles.len(), 1, "{name} has one profile");
        &profiles[0]
    }

    fn assert_rect(r: Rect, min: (f64, f64), max: (f64, f64), tol: f64) {
        assert_approx_eq!(r.min.x, min.0, tol);
        assert_approx_eq!(r.min.y, min.1, tol);
        assert_approx_eq!(r.max.x, max.0, tol);
        assert_approx_eq!(r.max.y, max.1, tol);
    }

    fn skipped(warnings: &[ImportWarning]) -> Option<&BTreeMap<String, usize>> {
        warnings.iter().find_map(|w| match w {
            ImportWarning::DxfEntitiesSkipped { counts } => Some(counts),
            _ => None,
        })
    }

    /// Every vertex plus three interior points of every segment, following
    /// arcs; enough to catch a segment that leaves the curve it should lie on.
    fn sample(poly: &Polyline) -> Vec<Point> {
        let mut points: Vec<Point> = poly.vertices().iter().map(|v| v.point()).collect();
        for seg in poly.segments() {
            let len = seg.length_mm();
            for k in 1..4 {
                if let Some((p, _)) = seg.point_at(len * f64::from(k) / 4.0) {
                    points.push(p);
                }
            }
        }
        points
    }

    fn distance_to_segment(p: Point, seg: &Segment) -> f64 {
        match seg.arc() {
            Some(arc) => {
                let v = p - arc.centre;
                let angle = v.y.atan2(v.x);
                let from_start = if arc.sweep_rad > 0.0 {
                    (angle - arc.start_angle_rad).rem_euclid(TAU)
                } else {
                    (arc.start_angle_rad - angle).rem_euclid(TAU)
                };
                if from_start <= arc.sweep_rad.abs() {
                    (v.length() - arc.radius_mm).abs()
                } else {
                    p.distance_to(seg.start).min(p.distance_to(seg.end))
                }
            }
            None => {
                let d = seg.end - seg.start;
                let len2 = d.dot(d);
                if len2 <= 0.0 {
                    return p.distance_to(seg.start);
                }
                let t = ((p - seg.start).dot(d) / len2).clamp(0.0, 1.0);
                p.distance_to(seg.start + d * t)
            }
        }
    }

    fn distance_to_polyline(p: Point, poly: &Polyline) -> f64 {
        poly.segments()
            .map(|seg| distance_to_segment(p, &seg))
            .fold(f64::INFINITY, f64::min)
    }

    /// The furthest any sampled point of `a` lies from `b`, or of `b` from
    /// `a`: a sampled Hausdorff distance, mm.
    fn deviation_mm(a: &Polyline, b: &Polyline) -> f64 {
        let one_way = |from: &Polyline, to: &Polyline| {
            sample(from)
                .into_iter()
                .map(|p| distance_to_polyline(p, to))
                .fold(0.0, f64::max)
        };
        one_way(a, b).max(one_way(b, a))
    }

    // ── units ────────────────────────────────────────────────────────────

    #[test]
    fn insunits_mm_inch_cm_m_scale_correctly() {
        let cases: [(&[u8], DxfUnits, f64); 4] = [
            (fixture!("units_mm.dxf"), DxfUnits::Millimetres, 1.0),
            (fixture!("units_inch.dxf"), DxfUnits::Inches, 25.4),
            (fixture!("units_cm.dxf"), DxfUnits::Centimetres, 10.0),
            (fixture!("units_m.dxf"), DxfUnits::Metres, 1000.0),
        ];
        for (bytes, units, mm) in cases {
            let (result, warnings) = import(bytes);
            assert!(warnings.is_empty(), "{units}: {warnings:?}");
            assert_eq!(result.units, Some(units));
            assert_approx_eq!(result.mm_per_unit, mm, TOL_MM);
            // The fixture is one LINE from (0, 0) to (1, 0) in drawing units.
            let line = only(&result, "0");
            assert!(!line.is_closed(), "{units}: a LINE is open");
            assert_approx_eq!(line.length_mm(), mm, TOL_MM);
            assert_rect(line.bounds().unwrap(), (0.0, 0.0), (mm, 0.0), TOL_MM);
        }
    }

    #[test]
    fn missing_insunits_is_error_without_override() {
        let err = import_dxf(fixture!("no_units.dxf"), &DxfImportOptions::new()).unwrap_err();
        match err {
            ImportError::UnitsUnknown { declared } => assert_eq!(declared, "unitless"),
            other => panic!("unexpected error {other:?}"),
        }
        let (result, _) = import_with(
            fixture!("no_units.dxf"),
            DxfImportOptions {
                units: Some(Units::Mm),
                ..DxfImportOptions::new()
            },
        );
        assert_eq!(result.units, None);
        assert_approx_eq!(only(&result, "0").length_mm(), 1.0, TOL_MM);
    }

    #[test]
    fn units_override_wins() {
        let (result, _) = import_with(
            fixture!("units_mm.dxf"),
            DxfImportOptions {
                units: Some(Units::Inch),
                ..DxfImportOptions::new()
            },
        );
        assert_eq!(
            result.units,
            Some(DxfUnits::Millimetres),
            "the file's units are reported"
        );
        assert_approx_eq!(result.mm_per_unit, 25.4, TOL_MM);
        assert_approx_eq!(only(&result, "0").length_mm(), 25.4, TOL_MM);

        let (result, _) = import_with(
            fixture!("units_inch.dxf"),
            DxfImportOptions {
                units: Some(Units::Mm),
                ..DxfImportOptions::new()
            },
        );
        assert_approx_eq!(only(&result, "0").length_mm(), 1.0, TOL_MM);
    }

    // ── entities ─────────────────────────────────────────────────────────

    #[test]
    fn line_arc_circle_lwpolyline_import() {
        let (result, warnings) = import(fixture!("entities.dxf"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(names(&result), ["line", "arc", "circle", "lwpolyline"]);

        let line = only(&result, "line");
        assert!(!line.is_closed());
        assert_eq!(line.vertices().len(), 2);
        assert!(line.segments().all(|s| s.is_line()));
        assert_rect(line.bounds().unwrap(), (0.0, 0.0), (10.0, 0.0), TOL_MM);

        let arc = only(&result, "arc");
        assert!(!arc.is_closed());
        assert_eq!(arc.vertices().len(), 2);

        let circle = only(&result, "circle");
        assert!(circle.is_closed());
        assert_eq!(circle.vertices().len(), 2);

        let poly = only(&result, "lwpolyline");
        assert!(poly.is_closed());
        assert_eq!(poly.vertices().len(), 4);
    }

    /// The ARC is centred on (10, 10) with radius 5 from 0° to 90°. It
    /// imports as one bulge segment with that centre and radius, not as
    /// sampled lines (functional spec, capability 20: "arcs stay arcs").
    #[test]
    fn arc_keeps_centre_and_radius_as_bulge() {
        let (result, _) = import(fixture!("entities.dxf"));
        let poly = only(&result, "arc");
        let seg = poly.segments().next().unwrap();
        let arc = seg.arc().expect("the segment is an arc");
        assert_approx_eq!(arc.centre.x, 10.0, TOL_MM);
        assert_approx_eq!(arc.centre.y, 10.0, TOL_MM);
        assert_approx_eq!(arc.radius_mm, 5.0, TOL_MM);
        assert_approx_eq!(arc.start_angle_rad, 0.0, TOL_MM);
        assert_approx_eq!(arc.sweep_rad, FRAC_PI_2, TOL_MM);
        assert_approx_eq!(poly.vertices()[0].bulge, (FRAC_PI_2 / 4.0).tan(), TOL_MM);
        assert_approx_eq!(seg.start.x, 15.0, TOL_MM);
        assert_approx_eq!(seg.start.y, 10.0, TOL_MM);
        assert_approx_eq!(seg.end.x, 10.0, TOL_MM);
        assert_approx_eq!(seg.end.y, 15.0, TOL_MM);
    }

    /// A CIRCLE at (30, 30) with radius 4 is the closed two-vertex bulge-1
    /// polyline at (cx ± r, cy), counter-clockwise, as `Polyline::circle`
    /// builds one.
    #[test]
    fn circle_is_two_bulge_arcs() {
        let (result, _) = import(fixture!("entities.dxf"));
        let circle = only(&result, "circle");
        assert_eq!(
            circle,
            &Polyline::circle(Point::new(30.0, 30.0), 4.0),
            "two bulge-1 vertices at (34, 30) and (26, 30)"
        );
        assert_approx_eq!(circle.signed_area_mm2(), PI * 16.0, 1e-9);
    }

    /// No Y flip (ADR-0001: DXF is Y-up like DreamCarve). A bulge of +0.5
    /// from (0, 20) to (10, 20) is a counter-clockwise arc whose middle
    /// dips below the chord, to y = 17.5; a flipped import would raise it.
    #[test]
    fn bulge_sign_is_not_flipped() {
        let (result, _) = import(fixture!("entities.dxf"));
        let poly = only(&result, "lwpolyline");
        assert_approx_eq!(poly.vertices()[0].bulge, 0.5, TOL_MM);
        let seg = poly.segments().next().unwrap();
        let arc = seg.arc().unwrap();
        assert!(arc.sweep_rad > 0.0, "counter-clockwise: {arc:?}");
        let (middle, _) = seg.point_at(seg.length_mm() / 2.0).unwrap();
        assert_approx_eq!(middle.x, 5.0, TOL_MM);
        assert_approx_eq!(middle.y, 17.5, TOL_MM);
    }

    #[test]
    fn polyline_with_vertex_entities() {
        let (result, warnings) = import(fixture!("polyline_vertex.dxf"));
        // The polyface mesh (flag 16) is skipped and counted.
        assert_eq!(names(&result), ["poly"]);
        let poly = only(&result, "poly");
        assert!(poly.is_closed());
        let bulges: Vec<f64> = poly.vertices().iter().map(|v| v.bulge).collect();
        assert_eq!(bulges.len(), 4);
        for (got, want) in bulges.iter().zip([1.0, 0.0, -0.5, 0.0]) {
            assert_approx_eq!(*got, want, TOL_MM);
        }
        assert_eq!(poly.vertices()[1].point(), Point::new(10.0, 0.0));
        let counts = skipped(&warnings).expect("the mesh is counted");
        assert_eq!(counts.get("POLYLINE (mesh)"), Some(&1));
    }

    #[test]
    fn closed_flag_closes_polyline() {
        let (closed, _) = import(fixture!("entities.dxf"));
        assert!(only(&closed, "lwpolyline").is_closed());
        let (open, _) = import(fixture!("lwpolyline_open.dxf"));
        let poly = only(&open, "lwpolyline");
        assert!(!poly.is_closed());
        assert_eq!(poly.vertices().len(), 4);
    }

    /// The cubic B-spline (knots 0 0 0 0 ½ 1 1 1 1, control points (0,0)
    /// (10,20) (20,−20) (30,20) (40,0)) is exactly two cubic Béziers: (0,0)
    /// (10,20) (15,0) (20,0) and (20,0) (25,0) (30,20) (40,0). The imported
    /// lines lie within the tolerance of them. The rational quadratic
    /// (weights 1, √½, 1) is exactly a quarter circle of radius 10 about
    /// (50, 50); its lines lie within the tolerance of that circle.
    #[test]
    fn spline_flattens_within_tolerance() {
        let options = DxfImportOptions::new();
        let tolerance = options.flatten_tolerance_mm;
        let (result, warnings) = import_with(fixture!("spline.dxf"), options);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(names(&result), ["spline", "rational"]);

        let spline = only(&result, "spline");
        assert!(!spline.is_closed());
        assert!(spline.segments().all(|s| s.is_line()), "flattened to lines");
        assert!(
            spline.vertices().len() > 8,
            "{} vertices",
            spline.vertices().len()
        );
        let first = spline.vertices()[0].point();
        let last = spline.vertices().last().unwrap().point();
        assert_approx_eq!(first.x, 0.0, TOL_MM);
        assert_approx_eq!(first.y, 0.0, TOL_MM);
        assert_approx_eq!(last.x, 40.0, TOL_MM);
        assert_approx_eq!(last.y, 0.0, TOL_MM);
        let beziers = [
            kurbo::CubicBez::new((0.0, 0.0), (10.0, 20.0), (15.0, 0.0), (20.0, 0.0)),
            kurbo::CubicBez::new((20.0, 0.0), (25.0, 0.0), (30.0, 20.0), (40.0, 0.0)),
        ];
        for p in sample(spline) {
            let distance = beziers
                .iter()
                .map(|b| {
                    kurbo::ParamCurveNearest::nearest(b, kurbo::Point::new(p.x, p.y), 1e-9)
                        .distance_sq
                        .sqrt()
                })
                .fold(f64::INFINITY, f64::min);
            assert!(
                distance <= tolerance + 1e-6,
                "{p:?} is {distance} mm from the spline"
            );
        }

        let quarter = only(&result, "rational");
        assert!(!quarter.is_closed());
        assert!(quarter.segments().all(|s| s.is_line()));
        let centre = Point::new(50.0, 50.0);
        for p in sample(quarter) {
            let err = (p.distance_to(centre) - 10.0).abs();
            assert!(err <= tolerance + 1e-6, "{p:?} is {err} mm off the circle");
        }
        let first = quarter.vertices()[0].point();
        let last = quarter.vertices().last().unwrap().point();
        assert_approx_eq!(first.x, 60.0, TOL_MM);
        assert_approx_eq!(first.y, 50.0, TOL_MM);
        assert_approx_eq!(last.x, 50.0, TOL_MM);
        assert_approx_eq!(last.y, 60.0, TOL_MM);
    }

    /// The ELLIPSE is centred on (50, 50), major axis (20, 0), ratio 0.5: a
    /// 40 × 20 mm ellipse. The half ellipse at (100, 50) has its major axis
    /// along +Y and runs from parameter 0 to π, through the minor axis on
    /// the −X side (the minor axis is 90° counter-clockwise from the major).
    #[test]
    fn ellipse_flattens_within_tolerance() {
        let options = DxfImportOptions::new();
        let tolerance = options.flatten_tolerance_mm;
        let (result, warnings) = import_with(fixture!("ellipse.dxf"), options);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(names(&result), ["ellipse", "half"]);

        let ellipse = only(&result, "ellipse");
        assert!(ellipse.is_closed());
        assert!(ellipse.segments().all(|s| s.is_line()));
        assert!(ellipse.vertices().len() > 8);
        assert_rect(
            ellipse.bounds().unwrap(),
            (30.0, 40.0),
            (70.0, 60.0),
            tolerance,
        );
        // Nearest point on the true ellipse, by a fine parametric sweep:
        // 40 000 steps on a ~97 mm perimeter leave gaps under 0.003 mm.
        let steps = 40_000;
        let on_ellipse: Vec<Point> = (0..steps)
            .map(|i| {
                let t = TAU * f64::from(i) / f64::from(steps);
                Point::new(50.0 + 20.0 * t.cos(), 50.0 + 10.0 * t.sin())
            })
            .collect();
        for p in sample(ellipse) {
            let err = on_ellipse
                .iter()
                .map(|q| p.distance_to(*q))
                .fold(f64::INFINITY, f64::min);
            assert!(
                err <= tolerance + 0.003,
                "{p:?} is {err} mm off the ellipse"
            );
        }

        let half = only(&result, "half");
        assert!(!half.is_closed());
        assert_rect(
            half.bounds().unwrap(),
            (95.0, 40.0),
            (100.0, 60.0),
            tolerance,
        );
        let first = half.vertices()[0].point();
        let last = half.vertices().last().unwrap().point();
        assert_approx_eq!(first.x, 100.0, TOL_MM);
        assert_approx_eq!(first.y, 60.0, TOL_MM);
        assert_approx_eq!(last.x, 100.0, TOL_MM);
        assert_approx_eq!(last.y, 40.0, TOL_MM);
    }

    // ── inserts ──────────────────────────────────────────────────────────

    /// Block `SQ` is a 10 mm square at the origin. Inserted at (100, 50)
    /// with scale 2 and rotation 90°, `T · R · S · T(−base)` maps (x, y) to
    /// (100 − 2y, 50 + 2x): the square spans x 80..100, y 50..70.
    #[test]
    fn insert_applies_translation_rotation_scale() {
        let (result, warnings) = import(fixture!("insert.dxf"));
        assert!(warnings.is_empty(), "{warnings:?}");
        let part = only(&result, "part");
        assert!(part.is_closed());
        assert_eq!(part.vertices().len(), 4);
        assert_rect(part.bounds().unwrap(), (80.0, 50.0), (100.0, 70.0), TOL_MM);
        // Rotation keeps the winding: still counter-clockwise.
        assert!(part.signed_area_mm2() > 0.0);
        assert_approx_eq!(part.signed_area_mm2(), 400.0, 1e-6);
    }

    /// Block `NEST` inserts `SQ` at (5, 0); `NEST` is inserted at (0, 100)
    /// at scale 2. Both transforms apply: x 10..30, y 100..120.
    #[test]
    fn nested_insert_accumulates() {
        let (result, _) = import(fixture!("insert.dxf"));
        let nested = only(&result, "nested");
        assert!(nested.is_closed());
        assert_rect(
            nested.bounds().unwrap(),
            (10.0, 100.0),
            (30.0, 120.0),
            TOL_MM,
        );
    }

    /// Block `ARCB` is a counter-clockwise quarter circle from (10, 0) to
    /// (0, 10) about the origin, as a bulge. Inserted with X scale −1 it is
    /// mirrored: it runs from (−10, 0) to (0, 10) clockwise, so the bulge
    /// is negated and the centre stays at the origin.
    #[test]
    fn mirrored_insert_negates_bulges() {
        let (result, _) = import(fixture!("insert_mirror.dxf"));
        let mirrored = only(&result, "mirrored");
        assert_eq!(mirrored.vertices().len(), 2);
        let b = (FRAC_PI_2 / 4.0).tan();
        assert_approx_eq!(mirrored.vertices()[0].bulge, -b, TOL_MM);
        assert_approx_eq!(mirrored.vertices()[0].x, -10.0, TOL_MM);
        assert_approx_eq!(mirrored.vertices()[0].y, 0.0, TOL_MM);
        let arc = mirrored.segments().next().unwrap().arc().unwrap();
        assert!(arc.sweep_rad < 0.0, "clockwise after the mirror");
        assert_approx_eq!(arc.centre.x, 0.0, TOL_MM);
        assert_approx_eq!(arc.centre.y, 0.0, TOL_MM);
        assert_approx_eq!(arc.radius_mm, 10.0, TOL_MM);
    }

    /// Under X scale 2, Y scale 1 the quarter circle becomes a quarter
    /// ellipse, which no bulge can hold: it is flattened to lines within the
    /// tolerance of the ellipse (x/2)² + y² = 100.
    #[test]
    fn non_uniform_scaled_arc_is_flattened() {
        let options = DxfImportOptions::new();
        let tolerance = options.flatten_tolerance_mm;
        let (result, _) = import_with(fixture!("insert_mirror.dxf"), options);
        let stretched = only(&result, "stretched");
        assert!(stretched.segments().all(|s| s.is_line()));
        assert!(stretched.vertices().len() > 4);
        assert_rect(
            stretched.bounds().unwrap(),
            (0.0, 0.0),
            (20.0, 10.0),
            tolerance,
        );
        for p in sample(stretched) {
            let err = (Vec2::new(p.x / 2.0, p.y).length() - 10.0).abs();
            // The implicit error is at most twice the distance to the curve.
            assert!(
                err <= 2.0 * tolerance + 1e-6,
                "{p:?} is {err} off the ellipse"
            );
        }
    }

    /// Block `LOOP` holds a line and an insert of itself one unit up. The
    /// expansion stops after `MAX_INSERT_DEPTH` levels with a warning
    /// naming the block, and what was expanded is kept.
    #[test]
    fn insert_depth_limit_warns() {
        let (result, warnings) = import(fixture!("insert_deep.dxf"));
        assert!(
            warnings.contains(&ImportWarning::DxfInsertTooDeep {
                block: "LOOP".into()
            }),
            "{warnings:?}"
        );
        // Levels 0 ..= MAX_INSERT_DEPTH each add a line at y = level; the
        // lines do not touch, so none stitch.
        let lines: Vec<_> = result
            .definitions
            .iter()
            .flat_map(|d| &d.profiles)
            .collect();
        assert_eq!(lines.len(), MAX_INSERT_DEPTH + 1);
        let top = lines
            .iter()
            .map(|p| p.bounds().unwrap().max.y)
            .fold(0.0, f64::max);
        assert_approx_eq!(top, MAX_INSERT_DEPTH as f64, TOL_MM);
    }

    /// Model- and paper-space blocks, blocks named with a leading `_`, the
    /// `AME_*`/`AVE_RENDER` solid-modeller blocks, inserts on the `ASHADE`,
    /// `HATCH` and `AME_*` layers, and blocks with no readable geometry are
    /// skipped silently, as upstream.
    #[test]
    fn system_blocks_and_layers_skipped() {
        let (result, warnings) = import(fixture!("system_blocks.dxf"));
        assert!(result.definitions.is_empty(), "{:?}", names(&result));
        assert!(result.instances.is_empty());
        assert_eq!(result.bounds, None);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    // ── spaces and skips ─────────────────────────────────────────────────

    #[test]
    fn paper_space_entities_skipped() {
        let (result, warnings) = import(fixture!("paper_space.dxf"));
        assert_eq!(names(&result), ["model"]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// Acceptance test 34: a file with TEXT and a DIMENSION imports its
    /// shapes and warns `dxf_entities_skipped` with `TEXT: 1, DIMENSION: 1`.
    #[test]
    fn unsupported_entities_counted_in_one_warning() {
        let (result, warnings) = import(fixture!("unsupported.dxf"));
        assert_eq!(names(&result), ["kept"]);
        let skipped_warnings: Vec<_> = warnings
            .iter()
            .filter(|w| matches!(w, ImportWarning::DxfEntitiesSkipped { .. }))
            .collect();
        assert_eq!(skipped_warnings.len(), 1, "{warnings:?}");
        let counts = skipped(&warnings).unwrap();
        let expected: BTreeMap<String, usize> = [("TEXT", 1), ("MTEXT", 1), ("DIMENSION", 1)]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();
        assert_eq!(counts, &expected);
        let text = skipped_warnings[0].to_string();
        assert!(text.contains("dxf_entities_skipped"), "{text}");
        assert!(
            text.contains("TEXT: 1") && text.contains("DIMENSION: 1"),
            "{text}"
        );
        assert!(
            warnings.contains(&ImportWarning::DxfBlockMissing {
                block: "MISSING".into()
            }),
            "{warnings:?}"
        );
    }

    // ── layers ───────────────────────────────────────────────────────────

    #[test]
    fn layer_names_become_shape_names() {
        let (result, _) = import(fixture!("layers.dxf"));
        assert_eq!(names(&result), ["a-1", "a-2", "b", "0"]);
        for (n, (inst, def)) in result.instances.iter().zip(&result.definitions).enumerate() {
            assert_eq!(def.id, DefinitionId::new(format!("d{}", n + 1)));
            assert_eq!(inst.id, InstanceId::new(format!("i{}", n + 1)));
            assert_eq!(inst.definition, def.id);
            assert_eq!(inst.name, def.name);
            assert_eq!(inst.transform, Transform::IDENTITY);
            assert_eq!(inst.z_top_mm, None);
            assert_eq!(inst.z_bottom_mm, None);
            assert!(inst.visible);
            assert!(!inst.locked);
        }
    }

    #[test]
    fn layer_filter_keeps_only_named_layers() {
        let filter = |layers: &[&str]| DxfImportOptions {
            layers: Some(layers.iter().map(|s| (*s).to_owned()).collect()),
            ..DxfImportOptions::new()
        };
        let (result, _) = import_with(fixture!("layers.dxf"), filter(&["a"]));
        assert_eq!(names(&result), ["a-1", "a-2"]);
        let (result, _) = import_with(fixture!("layers.dxf"), filter(&["0"]));
        assert_eq!(names(&result), ["0"]);
        let (result, _) = import_with(fixture!("layers.dxf"), filter(&["b", "0"]));
        assert_eq!(names(&result), ["b", "0"]);
        let (result, _) = import_with(fixture!("layers.dxf"), filter(&["none"]));
        assert!(result.definitions.is_empty());
        assert_eq!(result.bounds, None);
    }

    /// Block geometry on layer `0` belongs to the layer of the INSERT that
    /// places it, so the shapes are named `part` and `nested`, not `0`.
    #[test]
    fn block_entities_on_layer_0_inherit_insert_layer() {
        let (result, _) = import(fixture!("insert.dxf"));
        assert_eq!(names(&result), ["part", "nested"]);
    }

    // ── stitching and dedupe ─────────────────────────────────────────────

    /// Acceptance test 34: four LINE entities whose ends meet form one
    /// closed shape.
    #[test]
    fn four_lines_stitch_into_closed_shape() {
        let (result, warnings) = import(fixture!("stitch_closed.dxf"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(names(&result), ["box"]);
        let shape = only(&result, "box");
        assert!(shape.is_closed());
        assert_eq!(shape.vertices().len(), 4);
        assert_approx_eq!(shape.signed_area_mm2().abs(), 100.0, 1e-9);
        assert_rect(shape.bounds().unwrap(), (0.0, 0.0), (10.0, 10.0), TOL_MM);
    }

    /// Acceptance test 34: lines that leave a 0.5 mm gap stay one open
    /// chain at the default 0.01 mm tolerance; a 1 mm tolerance closes it.
    #[test]
    fn gap_beyond_tolerance_stays_open() {
        let (result, _) = import(fixture!("stitch_gap.dxf"));
        assert_eq!(names(&result), ["box"]);
        let chain = only(&result, "box");
        assert!(!chain.is_closed());
        assert_eq!(chain.vertices().len(), 5);
        assert_approx_eq!(chain.length_mm(), 39.5, TOL_MM);

        let (result, _) = import_with(
            fixture!("stitch_gap.dxf"),
            DxfImportOptions {
                join_tolerance_mm: 1.0,
                ..DxfImportOptions::new()
            },
        );
        let shape = only(&result, "box");
        assert!(shape.is_closed());
        assert_eq!(shape.vertices().len(), 4);
    }

    #[test]
    fn stitching_does_not_cross_layers() {
        let (result, _) = import(fixture!("stitch_layers.dxf"));
        assert_eq!(names(&result), ["a", "b"]);
        assert!(!only(&result, "a").is_closed());
        assert!(!only(&result, "b").is_closed());
        assert_eq!(only(&result, "a").vertices().len(), 2);
    }

    /// Two LINEs along the same points in opposite directions and two
    /// equal CIRCLEs on layer `dup` import once each; the same line on
    /// another layer is a shape of its own.
    #[test]
    fn identical_shapes_are_deduped() {
        let (result, _) = import(fixture!("dedupe.dxf"));
        assert_eq!(names(&result), ["dup-1", "dup-2", "other"]);
        assert!(!only(&result, "dup-1").is_closed());
        assert!(only(&result, "dup-2").is_closed());
    }

    // ── the test part ────────────────────────────────────────────────────

    /// Acceptance test 34: `plaque.dxf` imports to the same shapes as
    /// `plaque.svg`, named alike and in the same order, within 0.01 mm;
    /// the arcs keep the centres and radii the file states; the HOLE block's
    /// instances land at their insertion points at scale 2.5.
    #[test]
    fn plaque_dxf_matches_plaque_svg_within_0_01() {
        let (from_dxf, dxf_warnings) = import(include_bytes!("../../../tests/fixtures/plaque.dxf"));
        assert!(dxf_warnings.is_empty(), "{dxf_warnings:?}");
        let (from_svg, svg_warnings) = import_svg(
            include_bytes!("../../../tests/fixtures/plaque.svg"),
            &SvgImportOptions::new(),
        )
        .unwrap();
        assert!(svg_warnings.is_empty(), "{svg_warnings:?}");

        assert_eq!(
            names(&from_dxf),
            [
                "outline", "pocket", "island", "square", "holes-1", "holes-2", "holes-3",
                "holes-4", "border", "squiggle"
            ]
        );
        let svg_names: Vec<&str> = from_svg
            .definitions
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert_eq!(svg_names, names(&from_dxf));

        for (a, b) in from_dxf.definitions.iter().zip(&from_svg.definitions) {
            assert_eq!(a.profiles.len(), 1, "{}", a.name);
            assert_eq!(b.profiles.len(), 1, "{}", b.name);
            let (pa, pb) = (&a.profiles[0], &b.profiles[0]);
            assert_eq!(pa.is_closed(), pb.is_closed(), "{}", a.name);
            let deviation = deviation_mm(pa, pb);
            assert!(
                deviation <= 0.01,
                "{} differs by {deviation} mm between DXF and SVG",
                a.name
            );
        }

        // Arcs stay arcs with the stated centre and radius.
        let pocket = only(&from_dxf, "pocket");
        assert!(pocket.is_closed());
        let pocket_arcs: Vec<_> = pocket.segments().filter_map(|s| s.arc()).collect();
        assert_eq!(pocket_arcs.len(), 4, "four rounded corners");
        for arc in &pocket_arcs {
            assert_approx_eq!(arc.radius_mm, 5.0, TOL_MM);
            assert_approx_eq!(arc.sweep_rad, FRAC_PI_2, TOL_MM);
        }
        assert!(
            pocket_arcs
                .iter()
                .any(|a| a.centre.distance_to(Point::new(120.0, 47.0)) <= TOL_MM)
        );
        let island = only(&from_dxf, "island");
        assert_eq!(island, &Polyline::circle(Point::new(75.0, 67.0), 10.0));

        // The block's instances: insertion point and scale applied.
        for (name, centre) in [
            ("holes-1", Point::new(17.0, 17.0)),
            ("holes-2", Point::new(133.0, 17.0)),
            ("holes-3", Point::new(133.0, 83.0)),
            ("holes-4", Point::new(17.0, 83.0)),
        ] {
            let hole = only(&from_dxf, name);
            assert!(hole.is_closed());
            let arc = hole.segments().next().unwrap().arc().unwrap();
            assert_approx_eq!(arc.centre.x, centre.x, TOL_MM);
            assert_approx_eq!(arc.centre.y, centre.y, TOL_MM);
            assert_approx_eq!(arc.radius_mm, 2.5, TOL_MM);
        }

        assert_rect(from_dxf.bounds.unwrap(), (0.0, 0.0), (150.0, 100.0), TOL_MM);
    }

    // ── limits and errors ────────────────────────────────────────────────

    #[test]
    fn coordinate_limit_applies() {
        let err =
            import_dxf(fixture!("coordinate_limit.dxf"), &DxfImportOptions::new()).unwrap_err();
        match err {
            ImportError::CoordinateOutOfRange { path, limit_mm } => {
                assert_eq!(path, "far");
                assert_approx_eq!(limit_mm, MAX_COORDINATE_MM, 0.0);
            }
            other => panic!("unexpected error {other:?}"),
        }
        // The same line in inches reaches 177.8 m: still refused.
        let err = import_dxf(
            fixture!("coordinate_limit.dxf"),
            &DxfImportOptions {
                units: Some(Units::Inch),
                ..DxfImportOptions::new()
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, ImportError::CoordinateOutOfRange { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn invalid_options_are_rejected() {
        let bytes = fixture!("units_mm.dxf");
        for tolerance_mm in [-0.1, f64::NAN, f64::INFINITY] {
            let options = DxfImportOptions {
                join_tolerance_mm: tolerance_mm,
                ..DxfImportOptions::new()
            };
            assert!(matches!(
                import_dxf(bytes, &options),
                Err(ImportError::InvalidJoinTolerance { .. })
            ));
        }
        let options = DxfImportOptions {
            flatten_tolerance_mm: 0.0,
            ..DxfImportOptions::new()
        };
        assert!(matches!(
            import_dxf(bytes, &options),
            Err(ImportError::InvalidTolerance { .. })
        ));
        assert!(matches!(
            import_dxf(
                b"0\nSECTION\n2\nENTITIES\n0\nLINE\n10\nabc\n",
                &DxfImportOptions::new()
            ),
            Err(ImportError::Dxf(_))
        ));
    }

    #[test]
    fn import_is_deterministic() {
        let bytes = include_bytes!("../../../tests/fixtures/plaque.dxf");
        assert_eq!(import(bytes), import(bytes));
    }
}
