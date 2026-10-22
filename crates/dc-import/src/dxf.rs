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
//! The file is read by DreamCarve's own group-code reader (`groups`, then
//! `document`), ASCII and binary, as upstream parses it itself; every
//! group code read is cited from Autodesk's published DXF Reference. The
//! `$INSUNITS` table, the entity set, bulge arcs, the INSERT transform and
//! its skip rules, stitching with closing, and the dedupe follow
//! PureCutCNC. Changed from upstream: DXF is already Y-up, so there is no Y
//! flip and no arc-direction flip (ADR-0001); `ELLIPSE` is read; the
//! extrusion direction and MINSERT arrays are applied; the join tolerance
//! defaults to 0.01 mm instead of 0.5 mm; every skipped entity is counted
//! in a warning instead of dropped silently; arcs stay arcs (bulges)
//! instead of being sampled; and text before R2007 is read in the code
//! page `$DWGCODEPAGE` names (`code_page`, #209).

mod code_page;
mod document;
mod entities;
mod groups;
mod spline;
mod stitch;

use std::collections::BTreeMap;

use crate::{ImportError, ImportWarning};
use dc_geometry::{EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Rect, Transform};
use dc_project::{DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Units};

use crate::svg::{MAX_IMPORT_VERTICES, MIN_FLATTEN_TOLERANCE_MM};
use code_page::CodePage;
use entities::{Shape, Walker};

/// Why bytes are not a readable DXF file: a group that cannot be framed or
/// whose value does not fit its code's type (DXF Reference, "Group Code
/// Value Types"). Lines count from 1; binary offsets from the file's first
/// byte.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DxfReadError {
    /// An ASCII group code line that is not an integer.
    #[error("line {line}: `{text}` is not a group code")]
    BadCode {
        /// The line of the code.
        line: usize,
        /// What the line holds.
        text: String,
    },
    /// An ASCII value that does not parse as its code's type.
    #[error("line {line}: group code {code} needs a {expected}, not `{text}`")]
    BadValue {
        /// The line of the value.
        line: usize,
        /// The group code.
        code: i32,
        /// The type the code's value has.
        expected: &'static str,
        /// What the line holds.
        text: String,
    },
    /// The file ends after a group code, before its value.
    #[error("line {line}: group code {code} has no value; the file ends early")]
    MissingValue {
        /// The line of the code.
        line: usize,
        /// The group code.
        code: i32,
    },
    /// A binary file ends inside a group.
    #[error("binary DXF ends inside the group at byte {offset}")]
    TruncatedBinary {
        /// Where the cut-off value starts.
        offset: usize,
    },
    /// A binary group code the reference does not define, so its value's
    /// length is unknown.
    #[error("binary DXF has group code {code} at byte {offset}, which DXF does not define")]
    UnknownBinaryCode {
        /// The group code.
        code: i32,
        /// Where the code starts.
        offset: usize,
    },
}

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
/// [`ImportWarning::DxfEntitiesSkipped`]. Layer names are UTF-8, or before
/// R2007 in the code page `$DWGCODEPAGE` names; one without a table is
/// read as Windows-1252 with [`ImportWarning::DxfCodePageUnsupported`].
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

    let (document, mut warnings) = read_document(bytes)?;

    let declared = document.insunits.and_then(DxfUnits::from_insunits);
    let mm_per_unit = match (options.units, declared) {
        (Some(Units::Mm), _) => 1.0,
        (Some(Units::Inch), _) => 25.4,
        (None, Some(units)) => units.mm_per_unit(),
        (None, None) => {
            return Err(ImportError::UnitsUnknown {
                declared: insunits_name(document.insunits.unwrap_or(0)),
            });
        }
    };

    // No Y flip: DXF is Y-up (ADR-0001). The scale is the whole transform.
    let scale = Transform::scale(mm_per_unit, mm_per_unit);
    let mut walker = Walker::new(&document.blocks, tolerance_mm, options.max_vertices);
    for entity in &document.entities {
        if entity.in_paper_space() {
            continue;
        }
        walker.walk(entity, scale, None, 0)?;
    }

    let skipped = walker.skipped;
    if !skipped.is_empty() {
        warnings.push(ImportWarning::DxfEntitiesSkipped { counts: skipped });
    }
    for block in unique(walker.too_deep) {
        warnings.push(ImportWarning::DxfInsertTooDeep { block });
    }
    for block in unique(walker.missing) {
        warnings.push(ImportWarning::DxfBlockMissing { block });
    }

    let shapes: Vec<Shape> = walker
        .shapes
        .into_iter()
        .filter(|s| {
            options
                .layers
                .as_ref()
                .is_none_or(|layers| layers.contains(&s.layer))
        })
        .collect();
    let shapes = stitch::stitch(stitch::dedupe(shapes), join_mm);
    // A chain that closed on itself can enclose nothing (collinear lines).
    let shapes: Vec<Shape> = shapes
        .into_iter()
        .filter(|s| {
            let empty =
                s.polyline.is_closed() && s.polyline.signed_area_mm2().abs() <= EPS_MM * EPS_MM;
            if empty {
                warnings.push(ImportWarning::ZeroAreaDropped {
                    path: s.layer.clone(),
                    subpath: 0,
                });
            }
            !empty
        })
        .collect();

    // Names: the layer, numbered when the layer holds several shapes.
    let mut per_layer: BTreeMap<&str, usize> = BTreeMap::new();
    for shape in &shapes {
        *per_layer.entry(shape.layer.as_str()).or_insert(0) += 1;
    }
    let mut numbered: BTreeMap<&str, usize> = BTreeMap::new();
    let mut definitions = Vec::with_capacity(shapes.len());
    let mut instances = Vec::with_capacity(shapes.len());
    let mut bounds: Option<Rect> = None;
    for shape in &shapes {
        let name = if per_layer.get(shape.layer.as_str()).copied().unwrap_or(0) > 1 {
            let k = numbered.entry(shape.layer.as_str()).or_insert(0);
            *k += 1;
            format!("{}-{k}", shape.layer)
        } else {
            shape.layer.clone()
        };
        if let Some(b) = shape.polyline.bounds() {
            bounds = Some(match bounds {
                Some(acc) => acc.including(b.min).including(b.max),
                None => b,
            });
        }
        let n = definitions.len() + 1;
        let definition = DefinitionId::new(format!("d{n}"));
        instances.push(FeatureInstance {
            id: InstanceId::new(format!("i{n}")),
            definition: definition.clone(),
            transform: Transform::IDENTITY,
            name: name.clone(),
            visible: true,
            locked: false,
            z_top_mm: None,
            z_bottom_mm: None,
        });
        definitions.push(FeatureDefinition {
            id: definition,
            name,
            profiles: vec![shape.polyline.clone()],
        });
    }

    Ok((
        DxfImport {
            definitions,
            instances,
            bounds,
            units: declared,
            mm_per_unit,
        },
        warnings,
    ))
}

/// The document in `bytes`, its text read as UTF-8, else, before R2007,
/// in the code page `$DWGCODEPAGE` names (#209). The header is read in a
/// first pass, with Windows-1252; the file is read again only when some
/// text was not UTF-8 and the header names another code page. A code page
/// without a table is warned about only then, so a file whose text is all
/// ASCII gets no warning.
fn read_document(bytes: &[u8]) -> Result<(document::Document, Vec<ImportWarning>), ImportError> {
    let first = groups::read(bytes, CodePage::WINDOWS_1252)?;
    let document = document::read(&first.groups);
    if !first.not_utf8 || document.text_is_utf8() {
        return Ok((document, Vec::new()));
    }
    let Some(name) = document.dwgcodepage.clone() else {
        return Ok((document, Vec::new()));
    };
    match CodePage::from_dwgcodepage(&name) {
        Some(CodePage::WINDOWS_1252) => Ok((document, Vec::new())),
        Some(code_page) => {
            let groups = groups::read(bytes, code_page)?;
            Ok((document::read(&groups.groups), Vec::new()))
        }
        None => Ok((
            document,
            vec![ImportWarning::DxfCodePageUnsupported { code_page: name }],
        )),
    }
}

impl DxfUnits {
    /// The `$INSUNITS` value as units DreamCarve scales from; `None` for 0
    /// (unitless) and every unit outside the six.
    const fn from_insunits(code: i64) -> Option<Self> {
        match code {
            1 => Some(Self::Inches),
            2 => Some(Self::Feet),
            3 => Some(Self::Miles),
            4 => Some(Self::Millimetres),
            5 => Some(Self::Centimetres),
            6 => Some(Self::Metres),
            _ => None,
        }
    }
}

/// The name of an `$INSUNITS` value DreamCarve does not scale from, for
/// [`ImportError::UnitsUnknown`] (DXF Reference, "HEADER Section Group
/// Codes", `$INSUNITS`).
fn insunits_name(code: i64) -> String {
    let name = match code {
        0 => "unitless",
        7 => "kilometres",
        8 => "microinches",
        9 => "mils",
        10 => "yards",
        11 => "angstroms",
        12 => "nanometres",
        13 => "microns",
        14 => "decimetres",
        15 => "decametres",
        16 => "hectometres",
        17 => "gigametres",
        18 => "astronomical units",
        19 => "light years",
        20 => "parsecs",
        21 => "US survey feet",
        22 => "US survey inches",
        23 => "US survey yards",
        24 => "US survey miles",
        other => return format!("{other}, which DXF does not define"),
    };
    name.to_owned()
}

/// The first occurrence of each name, in order.
fn unique(names: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(names.len());
    for name in names {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
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

    // ── extrusion direction (OCS) ────────────────────────────────────────

    /// The first segment's arc of the one profile on `name`.
    fn first_arc(result: &DxfImport, name: &str) -> dc_geometry::Arc {
        only(result, name)
            .segments()
            .next()
            .and_then(|s| s.arc())
            .unwrap_or_else(|| panic!("{name} starts with an arc"))
    }

    /// Under extrusion (0, 0, −1) the arbitrary axis algorithm makes the
    /// OCS X axis world −X and keeps Y: every OCS point (x, y) is world
    /// (−x, y) and counter-clockwise about the normal is clockwise seen from
    /// +Z. The CIRCLE at OCS (10, 0) is centred on (−10, 0); the ARC about
    /// OCS (20, 5), radius 5, 0°–90° runs clockwise from (−25, 5) to
    /// (−20, 10); the LWPOLYLINE's bulge +0.5 becomes −0.5.
    #[test]
    fn negative_extrusion_mirrors_x() {
        let (result, _) = import(fixture!("extrusion.dxf"));

        let circle = first_arc(&result, "circle");
        assert_approx_eq!(circle.centre.x, -10.0, TOL_MM);
        assert_approx_eq!(circle.centre.y, 0.0, TOL_MM);
        assert_approx_eq!(circle.radius_mm, 3.0, TOL_MM);
        assert!(only(&result, "circle").is_closed());
        assert!(
            only(&result, "circle").signed_area_mm2() < 0.0,
            "the mirror reverses the winding"
        );
        // The same circle with normal (0, 0, +1) is not moved.
        assert_eq!(
            only(&result, "up"),
            &Polyline::circle(Point::new(10.0, 0.0), 3.0)
        );

        let poly = only(&result, "arc");
        assert!(!poly.is_closed());
        let b = (FRAC_PI_2 / 4.0).tan();
        assert_approx_eq!(poly.vertices()[0].x, -25.0, TOL_MM);
        assert_approx_eq!(poly.vertices()[0].y, 5.0, TOL_MM);
        assert_approx_eq!(poly.vertices()[0].bulge, -b, TOL_MM);
        assert_approx_eq!(poly.vertices()[1].x, -20.0, TOL_MM);
        assert_approx_eq!(poly.vertices()[1].y, 10.0, TOL_MM);
        let arc = first_arc(&result, "arc");
        assert_approx_eq!(arc.centre.x, -20.0, TOL_MM);
        assert_approx_eq!(arc.centre.y, 5.0, TOL_MM);
        assert_approx_eq!(arc.radius_mm, 5.0, TOL_MM);
        assert_approx_eq!(arc.sweep_rad, -FRAC_PI_2, TOL_MM);

        let lw = only(&result, "lwpolyline");
        assert!(!lw.is_closed());
        let got: Vec<(f64, f64, f64)> = lw.vertices().iter().map(|v| (v.x, v.y, v.bulge)).collect();
        for (got, want) in got
            .iter()
            .zip([(0.0, 0.0, -0.5), (-10.0, 0.0, 0.0), (-10.0, 10.0, 0.0)])
        {
            assert_approx_eq!(got.0, want.0, TOL_MM);
            assert_approx_eq!(got.1, want.1, TOL_MM);
            assert_approx_eq!(got.2, want.2, TOL_MM);
        }
        assert_eq!(got.len(), 3);
        // The arc's middle, (5, −2.5) in the OCS, is mirrored with it.
        let seg = lw.segments().next().unwrap();
        let (middle, _) = seg.point_at(seg.length_mm() / 2.0).unwrap();
        assert_approx_eq!(middle.x, -5.0, TOL_MM);
        assert_approx_eq!(middle.y, -2.5, TOL_MM);

        // The block's square is placed at OCS (100, 0): world x −110..−100,
        // wound clockwise.
        let insert = only(&result, "insert");
        assert_rect(
            insert.bounds().unwrap(),
            (-110.0, 0.0),
            (-100.0, 10.0),
            TOL_MM,
        );
        assert_approx_eq!(insert.signed_area_mm2(), -100.0, 1e-6);

        // An ELLIPSE's centre and major axis are world coordinates; the
        // normal only decides the side of the minor axis (N × major). The
        // half ellipse from parameter 0 to π runs from (20, 50) to
        // (−20, 50) through (0, 40), below the major axis.
        let tolerance = DxfImportOptions::new().flatten_tolerance_mm;
        let ellipse = only(&result, "ellipse");
        assert!(!ellipse.is_closed());
        assert_rect(
            ellipse.bounds().unwrap(),
            (-20.0, 40.0),
            (20.0, 50.0),
            tolerance,
        );
        let first = ellipse.vertices()[0].point();
        assert_approx_eq!(first.x, 20.0, TOL_MM);
        assert_approx_eq!(first.y, 50.0, TOL_MM);

        assert_eq!(
            names(&result),
            ["circle", "arc", "lwpolyline", "ellipse", "insert", "up"]
        );
    }

    /// A normal other than ±Z puts the entity in a plane DreamCarve cannot
    /// project without distorting it: it is skipped and counted, by type,
    /// as not in the XY plane.
    #[test]
    fn tilted_extrusion_is_skipped_with_warning() {
        let (result, warnings) = import(fixture!("extrusion.dxf"));
        assert!(!names(&result).contains(&"tilted"), "{:?}", names(&result));
        let counts = skipped(&warnings).expect("the tilted entities are counted");
        let expected: BTreeMap<String, usize> = [
            ("ARC (not in the XY plane)", 1),
            ("CIRCLE (not in the XY plane)", 1),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
        assert_eq!(counts, &expected);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }

    // ── MINSERT ──────────────────────────────────────────────────────────

    /// The lower-left corner of each profile on layer `layer`, sorted.
    fn corners(result: &DxfImport, layer: &str) -> Vec<(f64, f64)> {
        let mut corners: Vec<(f64, f64)> = result
            .definitions
            .iter()
            .filter(|d| d.name == layer || d.name.starts_with(&format!("{layer}-")))
            .flat_map(|d| &d.profiles)
            .map(|p| {
                let b = p.bounds().unwrap();
                ((b.min.x * 1e6).round() / 1e6, (b.min.y * 1e6).round() / 1e6)
            })
            .collect();
        corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
        corners
    }

    /// A MINSERT (INSERT with 70 = 3 columns, 71 = 2 rows, 44 = 20 column
    /// spacing, 45 = 10 row spacing) of a unit square at (100, 100) places
    /// six squares at (100 + 20c, 100 + 10r). The spacing is in drawing
    /// units, rotated with the insert but not scaled: the second array,
    /// rotated 90° at scale 2, steps its columns along +Y by 20.
    #[test]
    fn minsert_places_every_cell() {
        let (result, warnings) = import(fixture!("minsert.dxf"));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            names(&result),
            [
                "grid-1",
                "grid-2",
                "grid-3",
                "grid-4",
                "grid-5",
                "grid-6",
                "rotated-1",
                "rotated-2"
            ]
        );
        assert_eq!(
            corners(&result, "grid"),
            [
                (100.0, 100.0),
                (100.0, 110.0),
                (120.0, 100.0),
                (120.0, 110.0),
                (140.0, 100.0),
                (140.0, 110.0)
            ]
        );
        for n in 1..=6 {
            let square = only(&result, &format!("grid-{n}"));
            assert!(square.is_closed());
            assert_approx_eq!(square.signed_area_mm2(), 1.0, 1e-9);
        }
        assert_eq!(corners(&result, "rotated"), [(-2.0, 0.0), (-2.0, 20.0)]);
        for n in 1..=2 {
            assert_approx_eq!(
                only(&result, &format!("rotated-{n}")).signed_area_mm2(),
                4.0,
                1e-9
            );
        }
    }

    // ── POLYLINE fit vertices ────────────────────────────────────────────

    /// A spline-fit POLYLINE (flag 4) carries its frame control points
    /// (VERTEX flag 16) and the fitted curve's vertices (flag 8). Only the
    /// fitted vertices are the curve AutoCAD draws; the frame is dropped.
    #[test]
    fn spline_fit_polyline_drops_frame_vertices() {
        let (result, warnings) = import(fixture!("polyline_fit.dxf"));
        assert!(warnings.is_empty(), "{warnings:?}");
        let spline = only(&result, "spline");
        assert!(!spline.is_closed());
        let points: Vec<(f64, f64)> = spline.vertices().iter().map(|v| (v.x, v.y)).collect();
        assert_eq!(
            points,
            [
                (0.0, 0.0),
                (5.0, 7.0),
                (10.0, 9.0),
                (15.0, 7.0),
                (20.0, 0.0)
            ]
        );
        assert!(spline.segments().all(|s| s.is_line()));
    }

    /// A curve-fit POLYLINE (flag 2) keeps its own vertices and the extra
    /// vertices curve fitting added (VERTEX flag 1), with their bulges:
    /// together they are the fitted curve.
    #[test]
    fn curve_fit_polyline_keeps_every_vertex() {
        let (result, _) = import(fixture!("polyline_fit.dxf"));
        let curve = only(&result, "curve");
        let got: Vec<(f64, f64, f64)> = curve
            .vertices()
            .iter()
            .map(|v| (v.x, v.y, v.bulge))
            .collect();
        assert_eq!(got, [(0.0, 50.0, 0.2), (5.0, 53.0, 0.1), (10.0, 50.0, 0.0)]);
    }

    // ── the group-code reader ────────────────────────────────────────────

    /// What a group code's value is stored as in binary DXF, by the code
    /// ranges of the DXF reference's "Group Code Value Types" table; only
    /// the ranges the fixtures use.
    fn binary_kind(code: i32) -> char {
        match code {
            0..=9 | 100 | 102 | 105 | 300..=309 | 330..=369 | 999 | 1000..=1003 => 's',
            10..=59 | 110..=149 | 210..=239 | 1010..=1059 => 'd',
            60..=79 | 170..=179 | 270..=289 | 370..=389 | 1060..=1070 => 'h',
            90..=99 | 420..=429 | 1071 => 'i',
            290..=299 => 'b',
            310..=319 | 1004 => 'x',
            other => panic!("no binary kind for group code {other}"),
        }
    }

    /// Writes an ASCII DXF as binary DXF: the 22-byte sentinel, then each
    /// pair as a group code (2 bytes from R13 on, else 1 byte with 255
    /// escaping a 2-byte code) and its value (strings NUL-terminated,
    /// little-endian numbers, R13+ booleans as one byte, binary chunks as a
    /// length byte and the bytes).
    /// String values are copied byte for byte, so they may be in any code
    /// page.
    fn to_binary(ascii: &[u8], two_byte_codes: bool) -> Vec<u8> {
        let mut out = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
        let mut lines = ascii.split(|&b| b == b'\n').map(<[u8]>::trim_ascii);
        while let (Some(code), Some(value_bytes)) = (lines.next(), lines.next()) {
            let code: i32 = std::str::from_utf8(code).unwrap().parse().unwrap();
            let value = String::from_utf8_lossy(value_bytes);
            let code16 = u16::try_from(code).unwrap();
            if two_byte_codes {
                out.extend_from_slice(&code16.to_le_bytes());
            } else if code < 255 {
                out.push(u8::try_from(code).unwrap());
            } else {
                out.push(255);
                out.extend_from_slice(&code16.to_le_bytes());
            }
            match binary_kind(code) {
                's' => {
                    out.extend_from_slice(value_bytes);
                    out.push(0);
                }
                'd' => out.extend_from_slice(&value.parse::<f64>().unwrap().to_le_bytes()),
                'h' => out.extend_from_slice(&value.parse::<i16>().unwrap().to_le_bytes()),
                'i' => out.extend_from_slice(&value.parse::<i32>().unwrap().to_le_bytes()),
                'b' if two_byte_codes => out.push(value.parse::<u8>().unwrap()),
                'b' => out.extend_from_slice(&value.parse::<i16>().unwrap().to_le_bytes()),
                _ => {
                    let bytes: Vec<u8> = (0..value.len() / 2)
                        .map(|i| u8::from_str_radix(&value[2 * i..2 * i + 2], 16).unwrap())
                        .collect();
                    out.push(u8::try_from(bytes.len()).unwrap());
                    out.extend_from_slice(&bytes);
                }
            }
        }
        out
    }

    /// A LINE and a HATCH with a boundary path, an extended-data string and
    /// a binary chunk: the HATCH is not read and must be counted.
    const HATCH_DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
        0\nSECTION\n2\nENTITIES\n\
        0\nLINE\n8\nkept\n10\n0.0\n20\n0.0\n30\n0.0\n11\n10.0\n21\n0.0\n31\n0.0\n\
        0\nHATCH\n5\n2F\n330\n1F\n100\nAcDbEntity\n8\nfill\n100\nAcDbHatch\n\
        10\n0.0\n20\n0.0\n30\n0.0\n210\n0.0\n220\n0.0\n230\n1.0\n2\nSOLID\n70\n1\n71\n0\n\
        91\n1\n92\n2\n72\n0\n73\n1\n93\n0\n97\n0\n75\n0\n76\n1\n98\n0\n\
        1001\nACAD\n1000\nnote\n1004\n0A0B0C\n310\nDEADBEEF\n290\n1\n\
        0\nENDSEC\n0\nEOF\n";

    /// Issue #124: every entity type in the file is seen by the reader, so
    /// nothing is swallowed, in ASCII or in binary DXF (both code widths).
    #[test]
    fn swallowed_types_counted_in_binary_dxf() {
        let hatch = |result: &DxfImport, warnings: &[ImportWarning]| {
            assert_eq!(names(result), ["kept"]);
            let counts = skipped(warnings).expect("the HATCH is counted");
            assert_eq!(counts.get("HATCH"), Some(&1), "{counts:?}");
            assert_eq!(counts.len(), 1, "{counts:?}");
        };
        let (result, warnings) = import(HATCH_DXF.as_bytes());
        hatch(&result, &warnings);
        for two_byte_codes in [true, false] {
            let (result, warnings) = import(&to_binary(HATCH_DXF.as_bytes(), two_byte_codes));
            hatch(&result, &warnings);
        }
    }

    /// The plaque written as binary DXF, with R13+ two-byte codes and with
    /// R12 one-byte codes, imports exactly as the ASCII file does.
    #[test]
    fn binary_dxf_imports_like_ascii() {
        let ascii = include_bytes!("../../../tests/fixtures/plaque.dxf");
        let expected = import(ascii);
        for two_byte_codes in [true, false] {
            assert_eq!(
                import(&to_binary(ascii, two_byte_codes)),
                expected,
                "two-byte codes: {two_byte_codes}"
            );
        }
    }

    #[test]
    fn parser_rejects_odd_line_count_and_bad_codes() {
        let reject = |bytes: &[u8], what: &str| {
            let result = import_dxf(bytes, &DxfImportOptions::new());
            assert!(
                matches!(result, Err(ImportError::Dxf(_))),
                "{what}: {result:?}"
            );
        };
        // A group code with no value line after it.
        reject(b"0\nSECTION\n2\nENTITIES\n0\nLINE\n8", "odd line count");
        // A group code line that is not an integer.
        reject(b"0\nSECTION\n2\nENTITIES\n0\nLINE\nx8\nkept\n", "bad code");
        // A real-valued group whose value is not a number.
        reject(
            b"0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n40\n1..0\n",
            "bad real",
        );
        // A binary file cut off inside a double.
        let mut binary = to_binary(HATCH_DXF.as_bytes(), true);
        binary.truncate(binary.len() - 60);
        reject(&binary, "truncated binary");
        // A binary group code outside every range of the DXF reference.
        let mut unknown = b"AutoCAD Binary DXF\r\n\x1a\0".to_vec();
        unknown.extend_from_slice(&[0, 0]);
        unknown.extend_from_slice(b"SECTION\0");
        unknown.extend_from_slice(&85_u16.to_le_bytes());
        unknown.extend_from_slice(&[1, 2, 3, 4]);
        reject(&unknown, "unknown binary code");
    }

    // ── work limit ───────────────────────────────────────────────────────

    /// A DXF whose blocks are `blocks` (name, body entities) and whose
    /// ENTITIES section is `entities`.
    fn dxf_with_blocks(blocks: &[(&str, &str)], entities: &str) -> String {
        let mut dxf = String::from(
            "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n",
        );
        for (name, body) in blocks {
            dxf.push_str(&format!(
                "0\nBLOCK\n8\n0\n2\n{name}\n70\n0\n10\n0.0\n20\n0.0\n{body}0\nENDBLK\n"
            ));
        }
        dxf.push_str(&format!(
            "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
        ));
        dxf
    }

    /// A MINSERT of `block`, `n` × `n` cells 1 apart.
    fn minsert(block: &str, n: usize) -> String {
        format!(
            "0\nINSERT\n8\narray\n2\n{block}\n10\n0.0\n20\n0.0\n70\n{n}\n71\n{n}\n44\n1.0\n45\n1.0\n"
        )
    }

    /// A zero-length LINE: read, but it yields no vertices.
    const DOT: &str = "0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n0.0\n21\n0.0\n";

    fn assert_fails_fast(dxf: &str) {
        let start = std::time::Instant::now();
        let result = import_dxf(dxf.as_bytes(), &DxfImportOptions::new());
        let elapsed = start.elapsed();
        assert!(
            matches!(result, Err(ImportError::TooMuchWork { limit }) if limit == MAX_IMPORT_VERTICES / 10),
            "{result:?}"
        );
        assert!(elapsed.as_secs_f64() < 2.0, "took {elapsed:?}");
    }

    /// A 1000 × 1000 MINSERT of a block that yields no vertices would walk
    /// a million cells while the vertex limit never trips; it is refused
    /// before the first cell. A block with nothing readable at all is
    /// never expanded (upstream's rule), so it costs nothing.
    #[test]
    fn huge_minsert_of_empty_block_fails_fast() {
        assert_fails_fast(&dxf_with_blocks(&[("DOT", DOT)], &minsert("DOT", 1000)));
        let empty = dxf_with_blocks(&[("EMPTY", "")], &minsert("EMPTY", 1000));
        let start = std::time::Instant::now();
        let (result, _) = import(empty.as_bytes());
        assert!(result.definitions.is_empty());
        assert!(start.elapsed().as_secs_f64() < 1.0);
    }

    /// Three nested 100 × 100 MINSERTs are 10¹² visits of the innermost
    /// block; the per-visit count stops the walk at the limit.
    #[test]
    fn nested_minserts_fail_fast() {
        let inner = minsert("C", 100);
        let middle = minsert("B", 100);
        let dxf = dxf_with_blocks(
            &[("C", DOT), ("B", &inner), ("A", &middle)],
            &minsert("A", 100),
        );
        assert_fails_fast(&dxf);
    }

    /// An INSERT's attributes (ATTRIB, after `66 = 1`, up to SEQEND) are
    /// text and are counted, not dropped silently; the block is still
    /// placed.
    #[test]
    fn insert_attributes_are_counted() {
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
            0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\nTAG\n70\n2\n10\n0.0\n20\n0.0\n30\n0.0\n3\nTAG\n\
            0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n30\n0.0\n11\n5.0\n21\n0.0\n31\n0.0\n\
            0\nATTDEF\n8\n0\n10\n0.0\n20\n1.0\n30\n0.0\n40\n1.0\n1\n\n3\nName\n2\nNAME\n70\n0\n\
            0\nENDBLK\n8\n0\n0\nENDSEC\n\
            0\nSECTION\n2\nENTITIES\n\
            0\nINSERT\n8\npart\n66\n1\n2\nTAG\n10\n10.0\n20\n10.0\n30\n0.0\n\
            0\nATTRIB\n8\npart\n10\n10.0\n20\n11.0\n30\n0.0\n40\n1.0\n1\nbolt\n2\nNAME\n70\n0\n\
            0\nSEQEND\n8\npart\n0\nENDSEC\n0\nEOF\n";
        let (result, warnings) = import(dxf.as_bytes());
        assert_eq!(names(&result), ["part"]);
        assert_rect(
            only(&result, "part").bounds().unwrap(),
            (10.0, 10.0),
            (15.0, 10.0),
            TOL_MM,
        );
        let counts = skipped(&warnings).expect("the attribute is counted");
        assert_eq!(counts.get("ATTRIB"), Some(&1), "{counts:?}");
        assert_eq!(counts.get("ATTDEF"), Some(&1), "{counts:?}");
    }

    /// Before R2007 a DXF is written in the drawing's code page, usually
    /// Windows-1252; a layer name that is not UTF-8 is read in that code
    /// page. `\U+XXXX` escapes are decoded.
    #[test]
    fn non_utf8_names_are_read_as_windows_1252() {
        let mut dxf = b"0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n\
            0\nSECTION\n2\nENTITIES\n0\nLINE\n8\ncaf"
            .to_vec();
        dxf.push(0xE9);
        dxf.extend_from_slice(b" \x80\n10\n0.0\n20\n0.0\n11\n1.0\n21\n0.0\n");
        dxf.extend_from_slice(b"0\nLINE\n8\nd\\U+00E9cor\n10\n0.0\n20\n5.0\n11\n1.0\n21\n5.0\n");
        dxf.extend_from_slice(b"0\nENDSEC\n0\nEOF\n");
        let (result, _) = import(&dxf);
        assert_eq!(names(&result), ["caf\u{e9} \u{20ac}", "d\u{e9}cor"]);
    }

    /// A DXF with `$ACADVER` and `$DWGCODEPAGE` in its header and one LINE
    /// on each layer, whose names are given as raw bytes.
    fn dxf_in_code_page(acadver: &str, code_page: &str, layers: &[&[u8]]) -> Vec<u8> {
        let mut dxf = format!(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\n{acadver}\n9\n$DWGCODEPAGE\n3\n{code_page}\n\
            9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n"
        )
        .into_bytes();
        for (k, layer) in layers.iter().enumerate() {
            dxf.extend_from_slice(b"0\nLINE\n8\n");
            dxf.extend_from_slice(layer);
            dxf.extend_from_slice(format!("\n10\n0.0\n20\n{k}.0\n11\n1.0\n21\n{k}.0\n").as_bytes());
        }
        dxf.extend_from_slice(b"0\nENDSEC\n0\nEOF\n");
        dxf
    }

    /// Whether a warning names the code page `name`.
    fn warns_about_code_page(warnings: &[ImportWarning], name: &str) -> bool {
        warnings.iter().any(|w| w.to_string().contains(name))
    }

    /// Issue #209: an R2000 file saved in Windows-1251 (`$DWGCODEPAGE
    /// ANSI_1251`) has its layer names, and so its shape names, decoded in
    /// that code page, in ASCII and binary DXF alike. "Стены" and "Ёлка"
    /// in CP1251 bytes (Microsoft's `CP1251.TXT`).
    #[test]
    fn cp1251_layer_names_decode() {
        let walls: &[u8] = &[0xD1, 0xF2, 0xE5, 0xED, 0xFB];
        let tree: &[u8] = &[0xA8, 0xEB, 0xEA, 0xE0];
        let dxf = dxf_in_code_page("AC1015", "ANSI_1251", &[walls, tree]);
        for bytes in [dxf.clone(), to_binary(&dxf, true), to_binary(&dxf, false)] {
            let (result, warnings) = import(&bytes);
            assert_eq!(names(&result), ["Стены", "Ёлка"]);
            assert!(warnings.is_empty(), "{warnings:?}");
        }
    }

    /// Issue #209: a code page DreamCarve has no table for (here the
    /// multi-byte Shift-JIS, `ANSI_932`) is read as Windows-1252, as
    /// before, with a warning that names it. A file in such a code page
    /// whose text is all ASCII reads correctly and gets no warning.
    #[test]
    fn unknown_code_page_warns_and_falls_back() {
        // "壁" (wall) in Shift-JIS; as Windows-1252 bytes it reads "•Ç".
        let wall: &[u8] = &[0x95, 0xC7];
        let dxf = dxf_in_code_page("AC1015", "ANSI_932", &[wall]);
        let (result, warnings) = import(&dxf);
        assert_eq!(names(&result), ["\u{2022}\u{c7}"]);
        assert_eq!(
            warnings,
            [ImportWarning::DxfCodePageUnsupported {
                code_page: "ANSI_932".into()
            }]
        );
        assert!(warns_about_code_page(&warnings, "ANSI_932"), "{warnings:?}");
        for bytes in [to_binary(&dxf, true), to_binary(&dxf, false)] {
            assert_eq!(import(&bytes), (result.clone(), warnings.clone()));
        }

        let ascii = dxf_in_code_page("AC1015", "ANSI_932", &[b"walls"]);
        let (result, warnings) = import(&ascii);
        assert_eq!(names(&result), ["walls"]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// Issue #209: from R2007 (`AC1021`) on, text is UTF-8 whatever
    /// `$DWGCODEPAGE` says (a Russian AutoCAD writes `ANSI_1251` there), so
    /// such a file reads as before: UTF-8, else Windows-1252.
    #[test]
    fn utf8_file_is_unchanged() {
        let dxf = dxf_in_code_page("AC1027", "ANSI_1251", &["Стены".as_bytes()]);
        let (result, warnings) = import(&dxf);
        assert_eq!(names(&result), ["Стены"]);
        assert!(warnings.is_empty(), "{warnings:?}");

        // Not UTF-8: Windows-1252 as before, not the code page the header
        // names (0xE9 is "й" in CP1251).
        let dxf = dxf_in_code_page("AC1021", "ANSI_1251", &[b"caf\xE9"]);
        let (result, warnings) = import(&dxf);
        assert_eq!(names(&result), ["caf\u{e9}"]);
        assert!(warnings.is_empty(), "{warnings:?}");
    }
}
