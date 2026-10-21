// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/dxf.ts (instantiateEntity, shouldExpandInsert,
//   insertTransform, resolveEntityLayer, isModelSpaceEntity,
//   lwPolylineProfile, polylineProfile, arcProfile, circleEntityProfile,
//   lineEntityProfile, splineEntityProfile)

//! Entity to polyline conversion and INSERT expansion
//! (`docs/specs/phase-2/design.md`, "Algorithms → DXF import", steps 2–4).
//! Each entity becomes a bulge polyline in its own coordinates and is then
//! taken through the accumulated transform (block inserts and the unit
//! scale) into millimetres. Changed from upstream: no Y flip and no arc
//! flip (DXF is Y-up, ADR-0001); arcs stay bulges instead of being
//! sampled, and are flattened only under a transform no bulge can carry;
//! `ELLIPSE` is read; skipped entities are counted, not dropped.

use std::collections::BTreeMap;

use ::dxf::entities::{Entity, EntityType};
use ::dxf::{Block, Point as DxfPoint};
use dc_geometry::{
    EPS_MM, GeometryError, MAX_COORDINATE_MM, Point, Polyline, Transform, Vec2, Vertex, flatten,
};
use kurbo::{BezPath, PathEl, Shape as _};

use super::MAX_INSERT_DEPTH;
use super::spline::BSpline;
use crate::ImportError;
use crate::svg::{element_is_finite, element_is_within_limit, estimated_vertices};

/// A shape in millimetres with the layer it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Shape {
    /// The resolved layer name: the entity's, or the INSERT's for block
    /// geometry on layer `0`.
    pub layer: String,
    pub polyline: Polyline,
}

/// Inserts on these layers are the shading and hatch helpers of old
/// AutoCAD solid modellers; skipped silently, as upstream.
const IGNORED_INSERT_LAYERS: [&str; 2] = ["ASHADE", "HATCH"];
/// Blocks of the AME/AVE solid modellers; skipped silently, as upstream.
const IGNORED_INSERT_BLOCKS: [&str; 3] = ["AME_NIL", "AME_SOL", "AVE_RENDER"];

/// Collects the shapes of a drawing, in document order.
pub(super) struct Walker<'a> {
    blocks: BTreeMap<&'a str, &'a Block>,
    tolerance_mm: f64,
    max_vertices: usize,
    vertex_count: usize,
    pub shapes: Vec<Shape>,
    /// Skipped entities per type name.
    pub skipped: BTreeMap<String, usize>,
    /// Blocks whose INSERT nested past [`MAX_INSERT_DEPTH`], in order.
    pub too_deep: Vec<String>,
    /// Blocks an INSERT named that the file does not define, in order.
    pub missing: Vec<String>,
}

impl<'a> Walker<'a> {
    pub(super) fn new(
        blocks: impl Iterator<Item = &'a Block>,
        tolerance_mm: f64,
        max_vertices: usize,
    ) -> Self {
        Self {
            blocks: blocks.map(|b| (b.name.as_str(), b)).collect(),
            tolerance_mm,
            max_vertices,
            vertex_count: 0,
            shapes: Vec::new(),
            skipped: BTreeMap::new(),
            too_deep: Vec::new(),
            missing: Vec::new(),
        }
    }

    /// Adds the shapes of a model-space `entity` placed by `transform`
    /// (block inserts above it, then the unit scale). `inherited` is the
    /// layer of the INSERT whose block this entity belongs to; `depth` how
    /// many INSERTs deep it is.
    pub(super) fn walk(
        &mut self,
        entity: &Entity,
        transform: Transform,
        inherited: Option<&str>,
        depth: usize,
    ) -> Result<(), ImportError> {
        let layer = resolve_layer(&entity.common.layer, inherited);
        match &entity.specific {
            EntityType::Line(line) => {
                let polyline =
                    Polyline::new(vec![vertex(&line.p1, 0.0), vertex(&line.p2, 0.0)], false);
                self.add_polyline(polyline, transform, &layer, "LINE")
            }
            EntityType::Circle(circle) => {
                if !(circle.radius.is_finite() && circle.radius > 0.0) {
                    return self.skip("CIRCLE (degenerate)");
                }
                let polyline = Polyline::circle(point(&circle.center), circle.radius);
                self.add_polyline(polyline, transform, &layer, "CIRCLE")
            }
            EntityType::Arc(arc) => {
                if !(arc.radius.is_finite() && arc.radius > 0.0) {
                    return self.skip("ARC (degenerate)");
                }
                let polyline = arc_polyline(
                    point(&arc.center),
                    arc.radius,
                    arc.start_angle.to_radians(),
                    arc.end_angle.to_radians(),
                );
                self.add_polyline(polyline, transform, &layer, "ARC")
            }
            EntityType::LwPolyline(poly) => {
                let vertices = poly
                    .vertices
                    .iter()
                    .map(|v| Vertex::new(v.x, v.y, v.bulge))
                    .collect();
                let polyline = Polyline::new(vertices, poly.is_closed());
                self.add_polyline(polyline, transform, &layer, "LWPOLYLINE")
            }
            EntityType::Polyline(poly) => {
                if poly.is_3d_polygon_mesh() || poly.is_polyface_mesh() {
                    return self.skip("POLYLINE (mesh)");
                }
                let vertices = poly
                    .vertices()
                    .map(|v| vertex(&v.location, v.bulge))
                    .collect();
                let polyline = Polyline::new(vertices, poly.is_closed());
                self.add_polyline(polyline, transform, &layer, "POLYLINE")
            }
            EntityType::Spline(spline) => {
                let control: Vec<Point> = spline.control_points.iter().map(point).collect();
                let fit: Vec<Point> = spline.fit_points.iter().map(point).collect();
                if control.iter().chain(&fit).any(|p| !is_finite(*p)) {
                    return Err(ImportError::NonFinite { path: layer });
                }
                // B-splines are affine invariant: transforming the control
                // points first lets the sampling work in millimetres, where
                // the tolerance is defined.
                let control: Vec<Point> = control.iter().map(|p| transform.apply(*p)).collect();
                let fit: Vec<Point> = fit.iter().map(|p| transform.apply(*p)).collect();
                let degree = usize::try_from(spline.degree_of_curve).unwrap_or(0);
                let points = match BSpline::new(
                    degree,
                    &spline.knot_values,
                    &control,
                    &spline.weight_values,
                ) {
                    Some(curve) => curve.sample(self.tolerance_mm),
                    // Upstream's fallback: the fit points, else the control
                    // polygon.
                    None if fit.len() >= 2 => fit,
                    None => control,
                };
                let vertices = points.into_iter().map(|p| Vertex::line(p.x, p.y)).collect();
                let polyline = Polyline::new(vertices, spline.is_closed());
                self.add_mm_polyline(polyline, &layer, "SPLINE")
            }
            EntityType::Ellipse(ellipse) => {
                let centre = point(&ellipse.center);
                let major = Vec2::new(ellipse.major_axis.x, ellipse.major_axis.y);
                let values = [
                    centre.x,
                    centre.y,
                    major.x,
                    major.y,
                    ellipse.minor_axis_ratio,
                    ellipse.start_parameter,
                    ellipse.end_parameter,
                ];
                if values.iter().any(|v| !v.is_finite()) {
                    return Err(ImportError::NonFinite { path: layer });
                }
                let a = major.length();
                let b = a * ellipse.minor_axis_ratio;
                if a <= EPS_MM || b <= EPS_MM {
                    return self.skip("ELLIPSE (degenerate)");
                }
                let sweep = (ellipse.end_parameter - ellipse.start_parameter)
                    .rem_euclid(std::f64::consts::TAU);
                let full = sweep <= 1e-12;
                let arc = kurbo::Arc::new(
                    (centre.x, centre.y),
                    (a, b),
                    ellipse.start_parameter,
                    if full { std::f64::consts::TAU } else { sweep },
                    major.y.atan2(major.x),
                );
                // kurbo transforms the arc exactly (an ellipse under an
                // affine map is an ellipse), so the Bézier approximation and
                // the flattening both happen in millimetres.
                let mut path = (transform.as_kurbo() * arc).to_path(self.tolerance_mm);
                if full {
                    path.close_path();
                }
                self.add_path(path, &layer)
            }
            EntityType::Insert(insert) => {
                let Some(block) = self.blocks.get(insert.name.as_str()).copied() else {
                    self.missing.push(insert.name.clone());
                    return Ok(());
                };
                if !should_expand(&entity.common.layer, block) {
                    return Ok(());
                }
                if depth > MAX_INSERT_DEPTH {
                    self.too_deep.push(block.name.clone());
                    return Ok(());
                }
                let (sx, sy) = (insert.x_scale_factor, insert.y_scale_factor);
                let placement = [insert.location.x, insert.location.y, insert.rotation];
                if !(sx.is_finite() && sy.is_finite() && sx != 0.0 && sy != 0.0)
                    || placement.iter().any(|v| !v.is_finite())
                {
                    return self.skip("INSERT (degenerate)");
                }
                // T(insert) · R(rotation) · S(xscale, yscale) · T(−base).
                let local = Transform::translate(-block.base_point.x, -block.base_point.y)
                    .then(Transform::scale(sx, sy))
                    .then(Transform::rotate(insert.rotation.to_radians()))
                    .then(Transform::translate(insert.location.x, insert.location.y));
                let child_transform = local.then(transform);
                for child in &block.entities {
                    self.walk(child, child_transform, Some(&layer), depth + 1)?;
                }
                Ok(())
            }
            // Structural: VERTEX rows belong to their POLYLINE and SEQEND
            // ends an INSERT's attributes or a POLYLINE.
            EntityType::Vertex(_) | EntityType::Seqend(_) => Ok(()),
            other => self.skip(type_name(other)),
        }
    }

    /// Counts a skipped entity; `Ok` so a `walk` arm can return it.
    fn skip(&mut self, kind: &str) -> Result<(), ImportError> {
        *self.skipped.entry(kind.to_owned()).or_insert(0) += 1;
        Ok(())
    }

    /// Adds a bulge polyline given in entity coordinates, through
    /// `transform`. Arcs survive rotation, uniform scale, translation and
    /// mirroring (which negates their bulges); under anything else the
    /// polyline is flattened to lines within the tolerance.
    fn add_polyline(
        &mut self,
        polyline: Polyline,
        transform: Transform,
        layer: &str,
        kind: &str,
    ) -> Result<(), ImportError> {
        if polyline
            .vertices()
            .iter()
            .any(|v| !(is_finite(v.point()) && v.bulge.is_finite()))
        {
            return Err(ImportError::NonFinite {
                path: layer.to_owned(),
            });
        }
        let polyline = without_repeats(polyline);
        if polyline.vertices().len() < 2 {
            return self.skip(&format!("{kind} (degenerate)"));
        }
        match transform.apply_polyline(&polyline) {
            Ok(placed) => self.add_mm_polyline(placed, layer, kind),
            Err(GeometryError::ArcUnderNonUniformTransform) => {
                let path = transform.as_kurbo() * to_bez_path(&polyline, self.tolerance_mm);
                self.add_path(path, layer)
            }
            Err(GeometryError::CoordinateOutOfRange { limit_mm, .. }) => {
                Err(ImportError::CoordinateOutOfRange {
                    path: layer.to_owned(),
                    limit_mm,
                })
            }
            // A singular INSERT scale is refused before it gets here.
            Err(_) => self.skip(&format!("{kind} (degenerate)")),
        }
    }

    /// Adds a polyline already in millimetres.
    fn add_mm_polyline(
        &mut self,
        polyline: Polyline,
        layer: &str,
        kind: &str,
    ) -> Result<(), ImportError> {
        let polyline = without_repeats(polyline);
        let reach_mm = polyline.reach_mm();
        if !reach_mm.is_finite() {
            return Err(ImportError::NonFinite {
                path: layer.to_owned(),
            });
        }
        if reach_mm > MAX_COORDINATE_MM {
            return Err(ImportError::CoordinateOutOfRange {
                path: layer.to_owned(),
                limit_mm: MAX_COORDINATE_MM,
            });
        }
        let degenerate = polyline.vertices().len() < 2
            || polyline.length_mm() <= EPS_MM
            || (polyline.is_closed() && polyline.signed_area_mm2().abs() <= EPS_MM * EPS_MM);
        if degenerate {
            return self.skip(&format!("{kind} (degenerate)"));
        }
        self.count_vertices(polyline.vertices().len())?;
        self.shapes.push(Shape {
            layer: layer.to_owned(),
            polyline,
        });
        Ok(())
    }

    /// Flattens a Bézier path in millimetres and adds its subpaths.
    fn add_path(&mut self, path: BezPath, layer: &str) -> Result<(), ImportError> {
        if !path.elements().iter().all(element_is_finite) {
            return Err(ImportError::NonFinite {
                path: layer.to_owned(),
            });
        }
        if !path.elements().iter().all(element_is_within_limit) {
            return Err(ImportError::CoordinateOutOfRange {
                path: layer.to_owned(),
                limit_mm: MAX_COORDINATE_MM,
            });
        }
        // Refuse before flattening, so a runaway curve is never allocated.
        self.count_vertices(estimated_vertices(&path, self.tolerance_mm))?;
        for polyline in flatten(&path, self.tolerance_mm) {
            self.add_mm_polyline(polyline, layer, "flattened")?;
        }
        Ok(())
    }

    fn count_vertices(&mut self, added: usize) -> Result<(), ImportError> {
        self.vertex_count = self.vertex_count.saturating_add(added);
        if self.vertex_count > self.max_vertices {
            return Err(ImportError::TooManyVertices {
                limit: self.max_vertices,
            });
        }
        Ok(())
    }
}

/// The entity's layer, or the INSERT's for block geometry on layer `0`
/// (DXF's rule: layer 0 inside a block takes the insert's layer).
fn resolve_layer(own: &str, inherited: Option<&str>) -> String {
    match inherited {
        Some(layer) if own.is_empty() || own == "0" => layer.to_owned(),
        _ if own.is_empty() => "0".to_owned(),
        _ => own.to_owned(),
    }
}

/// Upstream's `shouldExpandInsert`: inserts on the solid-modeller layers,
/// of the model/paper space blocks, of blocks named with a leading `_`, of
/// the AME/AVE blocks, and of blocks holding nothing readable are skipped
/// silently.
fn should_expand(insert_layer: &str, block: &Block) -> bool {
    if insert_layer.starts_with("AME_") || IGNORED_INSERT_LAYERS.contains(&insert_layer) {
        return false;
    }
    let name = block.name.as_str();
    let lower = name.to_ascii_lowercase();
    if (lower.starts_with('*') || lower.starts_with('$'))
        && (lower.contains("model_space") || lower.contains("paper_space"))
    {
        return false;
    }
    if name.starts_with('_') || IGNORED_INSERT_BLOCKS.contains(&name) {
        return false;
    }
    block.entities.iter().any(|e| {
        matches!(
            e.specific,
            EntityType::Line(_)
                | EntityType::Arc(_)
                | EntityType::Circle(_)
                | EntityType::LwPolyline(_)
                | EntityType::Polyline(_)
                | EntityType::Spline(_)
                | EntityType::Ellipse(_)
                | EntityType::Insert(_)
        )
    })
}

/// The DXF type name of an entity DreamCarve does not read, for the
/// skipped-entity counts.
fn type_name(entity: &EntityType) -> &'static str {
    match entity {
        EntityType::Text(_) => "TEXT",
        EntityType::MText(_) => "MTEXT",
        EntityType::RotatedDimension(_)
        | EntityType::RadialDimension(_)
        | EntityType::DiameterDimension(_)
        | EntityType::AngularThreePointDimension(_)
        | EntityType::OrdinateDimension(_) => "DIMENSION",
        EntityType::Attribute(_) => "ATTRIB",
        EntityType::AttributeDefinition(_) => "ATTDEF",
        EntityType::Face3D(_) => "3DFACE",
        EntityType::Solid3D(_) => "3DSOLID",
        EntityType::ProxyEntity(_) => "ACAD_PROXY_ENTITY",
        EntityType::ArcAlignedText(_) => "ARCALIGNEDTEXT",
        EntityType::Body(_) => "BODY",
        EntityType::Helix(_) => "HELIX",
        EntityType::Image(_) => "IMAGE",
        EntityType::Leader(_) => "LEADER",
        EntityType::Light(_) => "LIGHT",
        EntityType::MLine(_) => "MLINE",
        EntityType::OleFrame(_) => "OLEFRAME",
        EntityType::Ole2Frame(_) => "OLE2FRAME",
        EntityType::ModelPoint(_) => "POINT",
        EntityType::Ray(_) => "RAY",
        EntityType::Region(_) => "REGION",
        EntityType::RText(_) => "RTEXT",
        EntityType::Section(_) => "SECTION",
        EntityType::Shape(_) => "SHAPE",
        EntityType::Solid(_) => "SOLID",
        EntityType::Tolerance(_) => "TOLERANCE",
        EntityType::Trace(_) => "TRACE",
        EntityType::DgnUnderlay(_) => "DGNUNDERLAY",
        EntityType::DwfUnderlay(_) => "DWFUNDERLAY",
        EntityType::PdfUnderlay(_) => "PDFUNDERLAY",
        EntityType::Wipeout(_) => "WIPEOUT",
        EntityType::XLine(_) => "XLINE",
        // Read by `walk`, never counted.
        EntityType::Line(_)
        | EntityType::Circle(_)
        | EntityType::Arc(_)
        | EntityType::LwPolyline(_)
        | EntityType::Polyline(_)
        | EntityType::Spline(_)
        | EntityType::Ellipse(_)
        | EntityType::Insert(_)
        | EntityType::Vertex(_)
        | EntityType::Seqend(_) => "ENTITY",
    }
}

/// The DXF type names the `dxf` crate parses. Any other entity type in
/// the file is swallowed by the parser before DreamCarve sees it, so the
/// importer counts those from the raw text.
pub(super) const PARSED_TYPE_NAMES: [&str; 45] = [
    "3DFACE",
    "3DSOLID",
    "ACAD_PROXY_ENTITY",
    "ARC",
    "ARCALIGNEDTEXT",
    "ATTDEF",
    "ATTRIB",
    "BODY",
    "CIRCLE",
    "DIMENSION",
    "ELLIPSE",
    "HELIX",
    "IMAGE",
    "INSERT",
    "LEADER",
    "LIGHT",
    "LINE",
    "3DLINE",
    "LWPOLYLINE",
    "MLINE",
    "MTEXT",
    "OLEFRAME",
    "OLE2FRAME",
    "POINT",
    "POLYLINE",
    "RAY",
    "REGION",
    "RTEXT",
    "SECTION",
    "SEQEND",
    "SHAPE",
    "SOLID",
    "SPLINE",
    "TEXT",
    "TOLERANCE",
    "TRACE",
    "DGNUNDERLAY",
    "DWFUNDERLAY",
    "PDFUNDERLAY",
    "VERTEX",
    "WIPEOUT",
    "XLINE",
    // Section and block bookkeeping, never entities.
    "ENDSEC",
    "EOF",
    "ENDBLK",
];

fn point(p: &DxfPoint) -> Point {
    Point::new(p.x, p.y)
}

fn vertex(p: &DxfPoint, bulge: f64) -> Vertex {
    Vertex::new(p.x, p.y, bulge)
}

fn is_finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// The ARC entity as a bulge polyline: DXF arcs run counter-clockwise from
/// the start angle to the end angle, so the sweep is normalised to (0, 2π].
/// A full turn (equal angles) is a circle, which one bulge cannot hold.
fn arc_polyline(centre: Point, radius_mm: f64, start_rad: f64, end_rad: f64) -> Polyline {
    let tau = std::f64::consts::TAU;
    let at = |angle: f64, bulge: f64| {
        Vertex::new(
            centre.x + radius_mm * angle.cos(),
            centre.y + radius_mm * angle.sin(),
            bulge,
        )
    };
    let sweep = (end_rad - start_rad).rem_euclid(tau);
    if sweep <= 1e-12 {
        return Polyline::new(
            vec![at(start_rad, 1.0), at(start_rad + tau / 2.0, 1.0)],
            true,
        );
    }
    Polyline::new(
        vec![at(start_rad, (sweep / 4.0).tan()), at(end_rad, 0.0)],
        false,
    )
}

/// Drops vertices that repeat their predecessor, and the last vertex of a
/// closed polyline when it repeats the first. A repeated vertex's bulge is
/// unused (its segment has no length), so dropping it changes nothing.
fn without_repeats(polyline: Polyline) -> Polyline {
    let closed = polyline.is_closed();
    let mut vertices: Vec<Vertex> = Vec::with_capacity(polyline.vertices().len());
    for v in polyline.vertices() {
        if vertices
            .last()
            .is_none_or(|last: &Vertex| last.point().distance_to(v.point()) > EPS_MM)
        {
            vertices.push(*v);
        }
    }
    if closed
        && vertices.len() > 1
        && let (Some(first), Some(last)) = (vertices.first(), vertices.last())
        && first.point().distance_to(last.point()) <= EPS_MM
    {
        vertices.pop();
    }
    Polyline::new(vertices, closed)
}

/// The polyline as a Bézier path, each arc as the cubics kurbo fits within
/// `tolerance_mm`, for flattening under a transform that bends arcs.
fn to_bez_path(polyline: &Polyline, tolerance_mm: f64) -> BezPath {
    let mut path = BezPath::new();
    let Some(first) = polyline.vertices().first() else {
        return path;
    };
    path.move_to((first.x, first.y));
    for seg in polyline.segments() {
        match seg.arc() {
            Some(arc) => {
                let kurbo_arc = kurbo::Arc::new(
                    (arc.centre.x, arc.centre.y),
                    (arc.radius_mm, arc.radius_mm),
                    arc.start_angle_rad,
                    arc.sweep_rad,
                    0.0,
                );
                for el in kurbo_arc.append_iter(tolerance_mm) {
                    path.push(el);
                }
            }
            None => path.line_to((seg.end.x, seg.end.y)),
        }
    }
    if polyline.is_closed() {
        path.push(PathEl::ClosePath);
    }
    path
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;
    use dc_geometry::assert_approx_eq;

    #[test]
    fn full_turn_arc_is_a_circle() {
        let circle = arc_polyline(Point::new(5.0, 5.0), 2.0, 0.0, 0.0);
        assert!(circle.is_closed());
        assert_eq!(circle.vertices().len(), 2);
        assert_approx_eq!(circle.signed_area_mm2(), std::f64::consts::PI * 4.0, 1e-9);
        let wrapped = arc_polyline(
            Point::new(5.0, 5.0),
            2.0,
            90.0_f64.to_radians(),
            450.0_f64.to_radians(),
        );
        assert!(wrapped.is_closed());
    }

    #[test]
    fn arc_crossing_zero_sweeps_the_short_way_counter_clockwise() {
        // 350° to 10° is a 20° counter-clockwise arc through 0°.
        let arc = arc_polyline(
            Point::new(0.0, 0.0),
            10.0,
            350.0_f64.to_radians(),
            10.0_f64.to_radians(),
        );
        let seg = arc.segments().next().unwrap();
        let a = seg.arc().unwrap();
        assert_approx_eq!(a.sweep_rad, 20.0_f64.to_radians(), 1e-9);
        assert_approx_eq!(a.radius_mm, 10.0, 1e-9);
    }

    #[test]
    fn layer_zero_inherits_and_others_do_not() {
        assert_eq!(resolve_layer("0", Some("part")), "part");
        assert_eq!(resolve_layer("", Some("part")), "part");
        assert_eq!(resolve_layer("walls", Some("part")), "walls");
        assert_eq!(resolve_layer("0", None), "0");
        assert_eq!(resolve_layer("", None), "0");
    }

    #[test]
    fn repeated_vertices_are_dropped() {
        let p = Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(0.0, 0.0),
                Vertex::line(10.0, 0.0),
                Vertex::line(10.0, 10.0),
                Vertex::line(0.0, 0.0),
            ],
            true,
        );
        let out = without_repeats(p);
        assert_eq!(out.vertices().len(), 3);
        assert!(out.is_closed());
    }
}
