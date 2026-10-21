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
//! taken through its object coordinate system and the accumulated
//! transform (block inserts and the unit scale) into millimetres. Changed
//! from upstream: no Y flip and no arc flip (DXF is Y-up, ADR-0001); arcs
//! stay bulges instead of being sampled, and are flattened only under a
//! transform no bulge can carry; `ELLIPSE` is read; the extrusion
//! direction is applied (−Z mirrors) and entities out of the XY plane are
//! counted; MINSERT arrays are expanded; a spline-fit POLYLINE drops its
//! frame vertices; skipped entities are counted, not dropped.

use std::collections::BTreeMap;

use dc_geometry::{
    EPS_MM, GeometryError, MAX_COORDINATE_MM, Point, Polyline, Transform, Vec2, Vertex, flatten,
};
use kurbo::{BezPath, PathEl, Shape as _};

use super::MAX_INSERT_DEPTH;
use super::document::{Block, Entity};
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
/// The entity types `walk` reads. A block holding none of them is not
/// expanded. `3DLINE` is the R12 name of a LINE with Z.
const READ_TYPES: [&str; 9] = [
    "LINE",
    "3DLINE",
    "ARC",
    "CIRCLE",
    "LWPOLYLINE",
    "POLYLINE",
    "SPLINE",
    "ELLIPSE",
    "INSERT",
];

/// Closed flag of LWPOLYLINE, POLYLINE and SPLINE (group 70).
const FLAG_CLOSED: i64 = 1;
/// POLYLINE flags (group 70, DXF Reference "POLYLINE"): 3D polyline, 3D
/// polygon mesh, polyface mesh.
const POLYLINE_3D: i64 = 8;
const POLYLINE_POLYGON_MESH: i64 = 16;
const POLYLINE_POLYFACE_MESH: i64 = 64;
/// VERTEX flag (group 70, DXF Reference "VERTEX"): spline frame control
/// point, a vertex of the frame a spline-fit POLYLINE was fitted to, not of
/// the curve.
const VERTEX_SPLINE_FRAME: i64 = 16;

/// A normalised extrusion direction whose X and Y are both within this of
/// zero is ±Z: the entity lies in the XY plane. The XY projection of its
/// OCS axes then differs from (±1, 0), (0, 1) by under 1e-12, which moves
/// no point within the coordinate limit measurably.
const PLANAR_NORMAL_EPS: f64 = 1e-6;

/// The block-entity placement limit is the vertex limit divided by this:
/// a placed entity is budgeted as ten vertices, generous for the circles,
/// arcs and short polylines that arrays repeat.
const MAX_VERTICES_PER_PLACEMENT: usize = 10;

/// Collects the shapes of a drawing, in document order.
pub(super) struct Walker<'a> {
    blocks: BTreeMap<&'a str, &'a Block>,
    tolerance_mm: f64,
    max_vertices: usize,
    vertex_count: usize,
    /// Block entities placed so far by INSERT and MINSERT expansion, once
    /// per placement, skipped entities included. Top-level entities are not
    /// counted: their number is bounded by the file's size.
    visits: usize,
    /// Most block-entity placements the import may make, a tenth of the
    /// vertex limit (500 000 by default). Nested arrays multiply: three
    /// nested 100 × 100 MINSERTs are 10¹² placements, and of a block that
    /// yields no vertices the vertex limit never stops them.
    max_visits: usize,
    pub shapes: Vec<Shape>,
    /// Skipped entities per type name.
    pub skipped: BTreeMap<String, usize>,
    /// Blocks whose INSERT nested past [`MAX_INSERT_DEPTH`], in order.
    pub too_deep: Vec<String>,
    /// Blocks an INSERT named that the file does not define, in order.
    pub missing: Vec<String>,
}

impl<'a> Walker<'a> {
    pub(super) fn new(blocks: &'a [Block], tolerance_mm: f64, max_vertices: usize) -> Self {
        Self {
            blocks: blocks.iter().map(|b| (b.name.as_str(), b)).collect(),
            tolerance_mm,
            max_vertices,
            vertex_count: 0,
            visits: 0,
            max_visits: max_vertices / MAX_VERTICES_PER_PLACEMENT,
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
    ///
    /// Group codes are those of the DXF Reference's entity pages: LINE
    /// 10/20 start, 11/21 end; CIRCLE and ARC 10/20 centre, 40 radius,
    /// 50/51 start and end angle (degrees); LWPOLYLINE 70 flags, 10/20
    /// vertex, 42 bulge; POLYLINE 70 flags with VERTEX 10/20 location, 42
    /// bulge, 70 flags; SPLINE 70 flags, 71 degree, 40 knots, 41 weights,
    /// 10/20 control points, 11/21 fit points. Common to all: 8 layer,
    /// 210/220/230 extrusion direction.
    pub(super) fn walk(
        &mut self,
        entity: &Entity,
        transform: Transform,
        inherited: Option<&str>,
        depth: usize,
    ) -> Result<(), ImportError> {
        if depth > 0 {
            self.visits = self.visits.saturating_add(1);
            if self.visits > self.max_visits {
                return Err(ImportError::TooMuchWork {
                    limit: self.max_visits,
                });
            }
        }
        let layer = resolve_layer(entity.layer(), inherited);
        let kind = entity.kind.as_str();
        match kind {
            // LINE and SPLINE points are world coordinates; their
            // extrusion only orients thickness and the curve's plane.
            "LINE" | "3DLINE" => {
                let polyline = Polyline::new(
                    vec![line_vertex(entity.point(10)), line_vertex(entity.point(11))],
                    false,
                );
                self.add_polyline(polyline, transform, &layer, "LINE")
            }
            "CIRCLE" => {
                let Some(ocs) = self.ocs(entity, kind) else {
                    return Ok(());
                };
                let radius = entity.real_or(40, 0.0);
                if !(radius.is_finite() && radius > 0.0) {
                    return self.skip("CIRCLE (degenerate)");
                }
                let polyline = Polyline::circle(entity.point(10), radius);
                self.add_polyline(polyline, ocs.then(transform), &layer, "CIRCLE")
            }
            "ARC" => {
                let Some(ocs) = self.ocs(entity, kind) else {
                    return Ok(());
                };
                let radius = entity.real_or(40, 0.0);
                if !(radius.is_finite() && radius > 0.0) {
                    return self.skip("ARC (degenerate)");
                }
                let polyline = arc_polyline(
                    entity.point(10),
                    radius,
                    entity.real_or(50, 0.0).to_radians(),
                    entity.real_or(51, 360.0).to_radians(),
                );
                self.add_polyline(polyline, ocs.then(transform), &layer, "ARC")
            }
            "LWPOLYLINE" => {
                let Some(ocs) = self.ocs(entity, kind) else {
                    return Ok(());
                };
                let mut vertices: Vec<Vertex> = Vec::new();
                for g in entity.groups() {
                    let value = g.real().unwrap_or(0.0);
                    match (g.code, vertices.last_mut()) {
                        (10, _) => vertices.push(Vertex::new(value, 0.0, 0.0)),
                        (20, Some(v)) => v.y = value,
                        (42, Some(v)) => v.bulge = value,
                        _ => {}
                    }
                }
                let polyline = Polyline::new(vertices, entity.int(70) & FLAG_CLOSED != 0);
                self.add_polyline(polyline, ocs.then(transform), &layer, "LWPOLYLINE")
            }
            "POLYLINE" => self.add_polyline_entity(entity, transform, &layer),
            "SPLINE" => {
                let control = entity.points(10);
                let fit = entity.points(11);
                if control.iter().chain(&fit).any(|p| !is_finite(*p)) {
                    return Err(ImportError::NonFinite { path: layer });
                }
                // B-splines are affine invariant: transforming the control
                // points first lets the sampling work in millimetres, where
                // the tolerance is defined.
                let control: Vec<Point> = control.iter().map(|p| transform.apply(*p)).collect();
                let fit: Vec<Point> = fit.iter().map(|p| transform.apply(*p)).collect();
                let degree = usize::try_from(entity.int(71)).unwrap_or(0);
                let knots = entity.reals(40);
                let weights = entity.reals(41);
                let points = match BSpline::new(degree, &knots, &control, &weights) {
                    Some(curve) => curve.sample(self.tolerance_mm),
                    // Upstream's fallback: the fit points, else the control
                    // polygon.
                    None if fit.len() >= 2 => fit,
                    None => control,
                };
                let vertices = points.into_iter().map(|p| Vertex::line(p.x, p.y)).collect();
                let polyline = Polyline::new(vertices, entity.int(70) & FLAG_CLOSED != 0);
                self.add_mm_polyline(polyline, &layer, "SPLINE")
            }
            "ELLIPSE" => self.add_ellipse(entity, transform, &layer),
            "INSERT" => self.expand_insert(entity, transform, &layer, depth),
            // Structural: VERTEX rows belong to their POLYLINE and SEQEND
            // ends an INSERT's attributes or a POLYLINE.
            "VERTEX" | "SEQEND" => Ok(()),
            other => self.skip(other),
        }
    }

    /// The map from the entity's object coordinate system to world XY, or
    /// `None` (and the entity counted) when it does not lie in the XY
    /// plane. See [`ocs_to_world`].
    fn ocs(&mut self, entity: &Entity, kind: &str) -> Option<Transform> {
        match ocs_to_world(entity.extrusion()) {
            Ok(ocs) => Some(ocs),
            Err(reason) => {
                *self
                    .skipped
                    .entry(format!("{kind} ({reason})"))
                    .or_insert(0) += 1;
                None
            }
        }
    }

    /// A POLYLINE and its VERTEX rows. Meshes are skipped. A 2D polyline's
    /// vertices are in its OCS, a 3D polyline's (flag 8) in world
    /// coordinates. A spline-fit polyline (flag 4) also holds the frame it
    /// was fitted to, as flag-16 vertices; the curve is the other vertices
    /// (flag 8), so the frame is dropped, unless the file holds nothing but
    /// the frame, which is then kept as SPLINE keeps its control polygon.
    /// A curve-fit polyline (flag 2) keeps every vertex: the ones curve
    /// fitting added (flag 1) and their bulges are the curve.
    fn add_polyline_entity(
        &mut self,
        entity: &Entity,
        transform: Transform,
        layer: &str,
    ) -> Result<(), ImportError> {
        let flags = entity.int(70);
        if flags & (POLYLINE_POLYGON_MESH | POLYLINE_POLYFACE_MESH) != 0 {
            return self.skip("POLYLINE (mesh)");
        }
        let ocs = if flags & POLYLINE_3D != 0 {
            Transform::IDENTITY
        } else {
            let Some(ocs) = self.ocs(entity, "POLYLINE") else {
                return Ok(());
            };
            ocs
        };
        let on_curve = |v: &Entity| v.int(70) & VERTEX_SPLINE_FRAME == 0;
        let only_frame = !entity.vertices.iter().any(on_curve);
        let vertices = entity
            .vertices
            .iter()
            .filter(|v| only_frame || on_curve(v))
            .map(|v| {
                let p = v.point(10);
                Vertex::new(p.x, p.y, v.real_or(42, 0.0))
            })
            .collect();
        let polyline = Polyline::new(vertices, flags & FLAG_CLOSED != 0);
        self.add_polyline(polyline, ocs.then(transform), layer, "POLYLINE")
    }

    /// An ELLIPSE (10/20 centre, 11/21 major axis end relative to the
    /// centre, 40 minor-to-major ratio, 41/42 start and end parameter).
    /// Centre and major axis are world coordinates; the extrusion decides
    /// the side of the minor axis (normal × major axis), so under −Z the
    /// parameter runs clockwise.
    fn add_ellipse(
        &mut self,
        entity: &Entity,
        transform: Transform,
        layer: &str,
    ) -> Result<(), ImportError> {
        let Some(ocs) = self.ocs(entity, "ELLIPSE") else {
            return Ok(());
        };
        let centre = entity.point(10);
        let major = Vec2::new(entity.real_or(11, 0.0), entity.real_or(21, 0.0));
        let ratio = entity.real_or(40, 1.0);
        let start = entity.real_or(41, 0.0);
        let end = entity.real_or(42, std::f64::consts::TAU);
        let values = [centre.x, centre.y, major.x, major.y, ratio, start, end];
        if values.iter().any(|v| !v.is_finite()) {
            return Err(ImportError::NonFinite {
                path: layer.to_owned(),
            });
        }
        let a = major.length();
        let b = a * ratio;
        if a <= EPS_MM || b <= EPS_MM {
            return self.skip("ELLIPSE (degenerate)");
        }
        let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
        let full = sweep <= 1e-12;
        let sweep = if full { std::f64::consts::TAU } else { sweep };
        // Under a mirrored OCS the minor axis is mirrored too: the point at
        // parameter t is the +Z ellipse's point at −t.
        let direction = if ocs == Transform::IDENTITY {
            1.0
        } else {
            -1.0
        };
        let arc = kurbo::Arc::new(
            (centre.x, centre.y),
            (a, b),
            direction * start,
            direction * sweep,
            major.y.atan2(major.x),
        );
        // kurbo transforms the arc exactly (an ellipse under an affine map
        // is an ellipse), so the Bézier approximation and the flattening
        // both happen in millimetres.
        let mut path = (transform.as_kurbo() * arc).to_path(self.tolerance_mm);
        if full {
            path.close_path();
        }
        self.add_path(path, layer)
    }

    /// An INSERT (2 block name, 10/20 insertion point, 41/42 X and Y
    /// scale, 50 rotation in degrees, 70/71 column and row count, 44/45
    /// column and row spacing): the block's entities placed by
    /// `T(insert) · R(rotation) · S(xscale, yscale) · T(−base)` in the
    /// insert's OCS, once per cell of a MINSERT array. Cell (column c, row
    /// r) is offset by (c · column spacing, r · row spacing), rotated with
    /// the insert and not scaled.
    fn expand_insert(
        &mut self,
        entity: &Entity,
        transform: Transform,
        layer: &str,
        depth: usize,
    ) -> Result<(), ImportError> {
        let name = entity.text(2).unwrap_or_default();
        let Some(block) = self.blocks.get(name).copied() else {
            self.missing.push(name.to_owned());
            return Ok(());
        };
        if !should_expand(entity.layer(), block) {
            return Ok(());
        }
        if depth > MAX_INSERT_DEPTH {
            self.too_deep.push(block.name.clone());
            return Ok(());
        }
        let Some(ocs) = self.ocs(entity, "INSERT") else {
            return Ok(());
        };
        let (sx, sy) = (entity.real_or(41, 1.0), entity.real_or(42, 1.0));
        let location = entity.point(10);
        let rotation_deg = entity.real_or(50, 0.0);
        let (dx, dy) = (entity.real_or(44, 0.0), entity.real_or(45, 0.0));
        let placement = [location.x, location.y, rotation_deg, dx, dy];
        if !(sx.is_finite() && sy.is_finite() && sx != 0.0 && sy != 0.0)
            || placement.iter().any(|v| !v.is_finite())
        {
            return self.skip("INSERT (degenerate)");
        }
        // Counts below 1 are the default single placement.
        let columns = entity.int(70).clamp(1, i64::from(i16::MAX));
        let rows = entity.int(71).clamp(1, i64::from(i16::MAX));
        // An array whose cells alone would pass the work limit is refused
        // before any cell is walked; nested arrays are caught by the
        // per-visit count in `walk`.
        let cells = usize::try_from(columns.saturating_mul(rows)).unwrap_or(usize::MAX);
        let visits = cells.saturating_mul(block.entities.len());
        if self.visits.saturating_add(visits) > self.max_visits {
            return Err(ImportError::TooMuchWork {
                limit: self.max_visits,
            });
        }
        let scale =
            Transform::translate(-block.base.x, -block.base.y).then(Transform::scale(sx, sy));
        let place = Transform::rotate(rotation_deg.to_radians())
            .then(Transform::translate(location.x, location.y))
            .then(ocs)
            .then(transform);
        for row in 0..rows {
            for column in 0..columns {
                let offset = Transform::translate(cell(column) * dx, cell(row) * dy);
                let child_transform = scale.then(offset).then(place);
                for child in &block.entities {
                    self.walk(child, child_transform, Some(layer), depth + 1)?;
                }
            }
        }
        Ok(())
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
    block
        .entities
        .iter()
        .any(|e| READ_TYPES.contains(&e.kind.as_str()))
}

/// The OCS of an entity with extrusion direction `normal`, as a map to
/// world XY, by the DXF Reference's arbitrary axis algorithm ("Object
/// Coordinate Systems (OCS)", "Arbitrary Axis Algorithm"): for a normal
/// near ±Z the OCS X axis is world Y × N and its Y axis N × X. For
/// N = (0, 0, 1) that is the world frame; for N = (0, 0, −1) X is world −X
/// and Y is world Y, a mirror in X that also reverses arcs (angles and
/// bulges turn counter-clockwise about N). Any other normal tilts the
/// entity out of the XY plane, and `Err` says so.
fn ocs_to_world(normal: [f64; 3]) -> Result<Transform, &'static str> {
    let [x, y, z] = normal;
    let length = (x * x + y * y + z * z).sqrt();
    if !(length.is_finite() && length > PLANAR_NORMAL_EPS) {
        return Err("degenerate");
    }
    if (x / length).abs() > PLANAR_NORMAL_EPS || (y / length).abs() > PLANAR_NORMAL_EPS {
        return Err("not in the XY plane");
    }
    Ok(if z > 0.0 {
        Transform::IDENTITY
    } else {
        Transform::scale(-1.0, 1.0)
    })
}

/// A MINSERT cell index (at most 32767) as a spacing multiplier.
fn cell(index: i64) -> f64 {
    i32::try_from(index).map_or(0.0, f64::from)
}

fn line_vertex(p: Point) -> Vertex {
    Vertex::line(p.x, p.y)
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

    /// The arbitrary axis algorithm for the two normals in the XY plane,
    /// a normal not quite of unit length, and normals out of the plane.
    #[test]
    fn ocs_of_plus_and_minus_z_and_tilted_normals() {
        assert_eq!(ocs_to_world([0.0, 0.0, 1.0]), Ok(Transform::IDENTITY));
        assert_eq!(ocs_to_world([-0.0, 1e-17, 2.0]), Ok(Transform::IDENTITY));
        let mirror = ocs_to_world([0.0, 0.0, -1.0]).unwrap();
        assert_eq!(mirror, Transform::scale(-1.0, 1.0));
        let p = mirror.apply(Point::new(3.0, 4.0));
        assert_approx_eq!(p.x, -3.0, 0.0);
        assert_approx_eq!(p.y, 4.0, 0.0);
        assert_eq!(ocs_to_world([0.0, 1.0, 0.0]), Err("not in the XY plane"));
        assert_eq!(ocs_to_world([1e-3, 0.0, 1.0]), Err("not in the XY plane"));
        assert_eq!(ocs_to_world([0.0, 0.0, 0.0]), Err("degenerate"));
        assert_eq!(ocs_to_world([f64::NAN, 0.0, 1.0]), Err("degenerate"));
    }
}
