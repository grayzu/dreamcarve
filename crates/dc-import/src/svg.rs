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
//!
//! Circular arcs stay arcs (#70, not in PureCutCNC, which flattens every
//! curve): usvg's Bézier pieces of circles, equal-radius ellipses,
//! rounded-rect corners and `A` commands are recognised exactly ([`arc`])
//! and become bulges, so generators offset true arcs and Drill sees exact
//! circles. Every other curve is flattened. The sweep is measured after
//! the flip, so a reflection reverses an arc's bulge by construction.

mod arc;

use crate::{ImportError, ImportWarning};
use arc::ArcPiece;
use dc_geometry::{
    EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, KURBO_FLATTEN_SAFETY, MAX_COORDINATE_MM, Point, Polyline,
    Rect, Transform, Vertex,
};
use dc_project::{DefinitionId, FeatureDefinition, FeatureInstance, InstanceId};
use kurbo::{Affine, BezPath, PathEl};
use usvg::tiny_skia_path::PathSegment;

/// Millimetres per CSS pixel. usvg resolves every length to pixels at 96
/// per inch, so this one factor converts any SVG to millimetres.
const MM_PER_PX: f64 = 25.4 / 96.0;

/// How close a subpath's last point must be to its start for the subpath
/// to count as closed without a `Z`, mm. usvg stores coordinates as f32,
/// whose rounding at workpiece scale is around 1e-5 mm; this sits above
/// that and far below anything a cutter can resolve.
const SUBPATH_CLOSE_TOLERANCE_MM: f64 = 1e-4;

const SVG_NS: &str = "http://www.w3.org/2000/svg";

/// Finest flattening tolerance an import accepts, mm (1 µm). Below any
/// machine's resolution; finer would only multiply vertices.
pub const MIN_FLATTEN_TOLERANCE_MM: f64 = 0.001;

/// Default cap on the total vertices one import may produce. A 90 mm circle
/// at the default tolerance is about 150 vertices; this leaves room for
/// very detailed artwork while stopping runaway input.
pub const MAX_IMPORT_VERTICES: usize = 5_000_000;

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
    /// At least [`MIN_FLATTEN_TOLERANCE_MM`].
    pub flatten_tolerance_mm: f64,
    /// Most vertices the whole import may produce before it fails with
    /// [`ImportError::TooManyVertices`].
    pub max_vertices: usize,
}

impl SvgImportOptions {
    /// Options at the default flattening tolerance,
    /// [`FLATTEN_TOLERANCE_DEFAULT_MM`], and the default vertex cap,
    /// [`MAX_IMPORT_VERTICES`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            flatten_tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
            max_vertices: MAX_IMPORT_VERTICES,
        }
    }
}

impl Default for SvgImportOptions {
    fn default() -> Self {
        Self::new()
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
    /// identity transform, `z_top_mm` and `z_bottom_mm` unset (`None`).
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
/// profiles are its subpaths, in mm, Y-up. Circular arcs (circles,
/// ellipses with rx = ry, rounded-rect corners with rx = ry, and `A`
/// commands with rx = ry, under a transform that keeps circles circular)
/// are bulge arcs of up to 180°, and a closed subpath that is one whole
/// circle is two half-circle arcs; every other curve is flattened to
/// `options.flatten_tolerance_mm`. Closed subpaths (ending in `Z`, or returning to their start point) become
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
    let tolerance_mm = options.flatten_tolerance_mm;
    if !(tolerance_mm.is_finite() && tolerance_mm >= MIN_FLATTEN_TOLERANCE_MM) {
        return Err(ImportError::InvalidTolerance { tolerance_mm });
    }

    let tree = usvg::Tree::from_data(bytes, &usvg_options())?;
    let root = inspect_root(bytes)?;

    let size = tree.size();
    let page_width_mm = f64::from(size.width()) * MM_PER_PX;
    let page_height_mm = f64::from(size.height()) * MM_PER_PX;
    if !(page_width_mm.is_finite()
        && page_height_mm.is_finite()
        && page_width_mm > 0.0
        && page_height_mm > 0.0
        && page_width_mm <= MAX_COORDINATE_MM
        && page_height_mm <= MAX_COORDINATE_MM)
    {
        return Err(ImportError::InvalidPageSize {
            width_mm: page_width_mm,
            height_mm: page_height_mm,
        });
    }
    let mm_per_user_unit = root
        .view_box_width
        .map_or(MM_PER_PX, |width| page_width_mm / width);

    let mut warnings = Vec::new();
    if root.view_box_width.is_none() && root.units != RootUnits::CssPixels {
        warnings.push(ImportWarning::PhysicalSizeWithoutViewBox { units: root.units });
    }
    if root.text_count > 0 {
        warnings.push(ImportWarning::TextIgnored {
            count: root.text_count,
        });
    }
    if root.image_count > 0 {
        warnings.push(ImportWarning::ImagesIgnored {
            count: root.image_count,
        });
    }

    let mut walker = Walker {
        tolerance_mm,
        // Pixels to millimetres, then the Y flip `y' = height − y`.
        px_to_mm: Affine::new([MM_PER_PX, 0.0, 0.0, -MM_PER_PX, 0.0, page_height_mm]),
        max_vertices: options.max_vertices,
        vertex_count: 0,
        path_count: 0,
        definitions: Vec::new(),
        instances: Vec::new(),
        bounds: None,
        warnings,
    };
    walker.walk_group(tree.root())?;

    let import = SvgImport {
        definitions: walker.definitions,
        instances: walker.instances,
        bounds: walker.bounds,
        page_width_mm,
        page_height_mm,
        units: root.units,
        mm_per_user_unit,
    };
    Ok((import, walker.warnings))
}

/// usvg options that keep the importer free of I/O: image references
/// resolve to nothing instead of being read from disk. Text needs usvg's
/// `text` feature, which is off, so usvg drops it; [`inspect_root`] counts
/// it for the warning.
fn usvg_options() -> usvg::Options<'static> {
    usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

/// What the raw document says that usvg's tree no longer records.
struct RootInfo {
    units: RootUnits,
    /// The `viewBox` width in user units, when there is a usable one.
    view_box_width: Option<f64>,
    text_count: usize,
    image_count: usize,
}

/// Reads the root's declared units and `viewBox`, and counts the text and
/// image elements usvg drops. Uses the same XML parser and options as usvg,
/// so it accepts exactly what usvg accepted.
fn inspect_root(bytes: &[u8]) -> Result<RootInfo, ImportError> {
    let text = std::str::from_utf8(bytes).map_err(|_| usvg::Error::NotAnUtf8Str)?;
    let xml_options = usvg::roxmltree::ParsingOptions {
        allow_dtd: true,
        ..usvg::roxmltree::ParsingOptions::default()
    };
    let doc = usvg::roxmltree::Document::parse_with_options(text, xml_options)
        .map_err(usvg::Error::ParsingFailed)?;
    let root = doc.root_element();

    let units = [root.attribute("width"), root.attribute("height")]
        .into_iter()
        .flatten()
        .map(length_units)
        .find(|u| *u != RootUnits::CssPixels)
        .unwrap_or(RootUnits::CssPixels);

    let view_box_width = root.attribute("viewBox").and_then(|vb| {
        let numbers: Vec<f64> = vb
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty())
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;
        match numbers.as_slice() {
            [_, _, width, height]
                if width.is_finite() && height.is_finite() && *width > 0.0 && *height > 0.0 =>
            {
                Some(*width)
            }
            _ => None,
        }
    });

    let count = |name: &str| {
        doc.descendants()
            .filter(|n| {
                n.is_element()
                    && n.tag_name().name() == name
                    && matches!(n.tag_name().namespace(), None | Some(SVG_NS))
            })
            .count()
    };

    Ok(RootInfo {
        units,
        view_box_width,
        text_count: count("text"),
        image_count: count("image"),
    })
}

/// The unit suffix of an SVG length such as `100mm` or `4.5in`.
fn length_units(length: &str) -> RootUnits {
    let length = length.trim();
    let number_len = length
        .trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%')
        .len();
    match length
        .get(number_len..)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("mm") => RootUnits::Millimetres,
        Some("cm") => RootUnits::Centimetres,
        Some("in") => RootUnits::Inches,
        Some("pt") => RootUnits::Points,
        Some("pc") => RootUnits::Picas,
        _ => RootUnits::CssPixels,
    }
}

/// Walks usvg's tree in document order, collecting definitions.
struct Walker {
    tolerance_mm: f64,
    px_to_mm: Affine,
    max_vertices: usize,
    /// Vertices flattened so far, across all paths.
    vertex_count: usize,
    /// Paths seen so far, imported or not; numbers unnamed paths.
    path_count: usize,
    definitions: Vec<FeatureDefinition>,
    instances: Vec<FeatureInstance>,
    bounds: Option<Rect>,
    warnings: Vec<ImportWarning>,
}

impl Walker {
    fn walk_group(&mut self, group: &usvg::Group) -> Result<(), ImportError> {
        if group.clip_path().is_some() {
            self.warnings.push(ImportWarning::ClipPathIgnored {
                group: group.id().to_owned(),
            });
        }
        if group.mask().is_some() {
            self.warnings.push(ImportWarning::MaskIgnored {
                group: group.id().to_owned(),
            });
        }
        for child in group.children() {
            match child {
                usvg::Node::Group(g) => self.walk_group(g)?,
                usvg::Node::Path(p) => self.import_path(p)?,
                // Counted from the raw document by `inspect_root`.
                usvg::Node::Image(_) | usvg::Node::Text(_) => {}
            }
        }
        Ok(())
    }

    fn import_path(&mut self, path: &usvg::Path) -> Result<(), ImportError> {
        self.path_count += 1;
        let name = if path.id().is_empty() {
            format!("path-{}", self.path_count)
        } else {
            path.id().to_owned()
        };

        let paints = [
            path.fill().map(usvg::Fill::paint),
            path.stroke().map(usvg::Stroke::paint),
        ];
        if paints.into_iter().flatten().any(|paint| {
            matches!(
                paint,
                usvg::Paint::LinearGradient(_)
                    | usvg::Paint::RadialGradient(_)
                    | usvg::Paint::Pattern(_)
            )
        }) {
            self.warnings
                .push(ImportWarning::PaintServerIgnored { path: name.clone() });
        }

        let to_mm = self.px_to_mm * affine(path.abs_transform());
        let mm_path = to_mm_path(path.data(), to_mm);
        let bez = &mm_path.bez;
        if !bez.elements().iter().all(element_is_finite) {
            return Err(ImportError::NonFinite { path: name });
        }
        // The curve lies inside its control polygon, so checking control
        // points bounds the whole path.
        if !bez.elements().iter().all(element_is_within_limit) {
            return Err(ImportError::CoordinateOutOfRange {
                path: name,
                limit_mm: MAX_COORDINATE_MM,
            });
        }
        // Refuse before flattening, so a runaway path is never allocated.
        let estimate = estimated_vertices(bez, self.tolerance_mm);
        if self.vertex_count.saturating_add(estimate) > self.max_vertices {
            return Err(ImportError::TooManyVertices {
                limit: self.max_vertices,
            });
        }
        let flattened = build_profiles(&mm_path, self.tolerance_mm);
        // The estimate is conservative; the exact count is what accumulates.
        for polyline in &flattened {
            self.vertex_count = self.vertex_count.saturating_add(polyline.vertices().len());
        }
        if self.vertex_count > self.max_vertices {
            return Err(ImportError::TooManyVertices {
                limit: self.max_vertices,
            });
        }

        let mut profiles: Vec<Polyline> = Vec::new();
        for (subpath, polyline) in flattened.into_iter().enumerate() {
            if polyline.is_closed() && polyline.signed_area_mm2().abs() <= EPS_MM * EPS_MM {
                self.warnings.push(ImportWarning::ZeroAreaDropped {
                    path: name.clone(),
                    subpath,
                });
                continue;
            }
            if let Some(b) = polyline.bounds() {
                self.bounds = Some(match self.bounds {
                    Some(acc) => acc.including(b.min).including(b.max),
                    None => b,
                });
            }
            profiles.push(polyline);
        }
        if profiles.is_empty() {
            self.warnings.push(ImportWarning::EmptyPath { path: name });
            return Ok(());
        }

        let n = self.definitions.len() + 1;
        let definition = DefinitionId::new(format!("d{n}"));
        self.instances.push(FeatureInstance {
            id: InstanceId::new(format!("i{n}")),
            definition: definition.clone(),
            transform: Transform::IDENTITY,
            name: name.clone(),
            visible: true,
            locked: false,
            z_top_mm: None,
            z_bottom_mm: None,
        });
        self.definitions.push(FeatureDefinition {
            id: definition,
            name,
            profiles,
        });
        Ok(())
    }
}

/// usvg's (tiny-skia's) transform as a kurbo affine. Both use the SVG
/// `matrix(a b c d e f)` layout.
fn affine(t: usvg::Transform) -> Affine {
    Affine::new([t.sx, t.ky, t.kx, t.sy, t.tx, t.ty].map(f64::from))
}

/// A path in millimetres, Y-up, and which of its elements are pieces of a
/// circular arc.
struct MmPath {
    bez: BezPath,
    /// One per element of `bez`: `Some` for the Bézier piece of a circular
    /// arc, which becomes a bulge instead of being flattened.
    arcs: Vec<Option<ArcHint>>,
}

/// What [`build_profiles`] needs to turn a recognised Bézier piece back
/// into its arc.
#[derive(Debug, Clone, Copy)]
struct ArcHint {
    /// Signed sweep in millimetre space (after the Y flip); positive is
    /// counter-clockwise.
    sweep_rad: f64,
    /// How far, mm, f32 rounding may have moved the piece's points.
    tolerance_mm: f64,
}

/// Converts a usvg path to a kurbo path through `to_mm`, marking the cubics
/// that are pieces of a circular arc (see [`arc`]). A subpath that ends
/// within [`SUBPATH_CLOSE_TOLERANCE_MM`] of its start without `Z` is
/// closed, its last point snapped onto the start, as PureCutCNC's
/// `parsePathProfiles` does.
fn to_mm_path(data: &usvg::tiny_skia_path::Path, to_mm: Affine) -> MmPath {
    let pt =
        |p: usvg::tiny_skia_path::Point| to_mm * kurbo::Point::new(f64::from(p.x), f64::from(p.y));
    // Only a similarity keeps a circle a circle; under anything else every
    // curve is flattened.
    let scale = arc::similarity_scale(to_mm);
    let mut out = MmPath {
        bez: BezPath::new(),
        arcs: Vec::new(),
    };
    let mut start = kurbo::Point::ZERO;
    let mut local_start = usvg::tiny_skia_path::Point::zero();
    let mut local_last = local_start;
    let mut open = false;
    for segment in data.segments() {
        let arc = match segment {
            PathSegment::MoveTo(p) => {
                close_if_returned(&mut out, start, open);
                start = pt(p);
                out.bez.move_to(start);
                (local_start, local_last) = (p, p);
                open = false;
                None
            }
            PathSegment::LineTo(p) => {
                out.bez.line_to(pt(p));
                local_last = p;
                open = true;
                None
            }
            PathSegment::QuadTo(c, p) => {
                out.bez.quad_to(pt(c), pt(p));
                local_last = p;
                open = true;
                None
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let local = [local_last, c1, c2, p];
                let [p0, c1, c2, p3] = local.map(pt);
                out.bez.curve_to(c1, c2, p3);
                local_last = p;
                open = true;
                scale.and_then(|scale| {
                    // f32 rounding is relative to the stored coordinates.
                    let largest = local
                        .iter()
                        .map(|q| f64::from(q.x.abs().max(q.y.abs())))
                        .fold(0.0, f64::max);
                    let tolerance_mm = arc::recognition_tolerance_mm(largest, scale);
                    arc::cubic_arc_sweep_rad(p0, c1, c2, p3, tolerance_mm).map(|sweep_rad| {
                        ArcHint {
                            sweep_rad,
                            tolerance_mm,
                        }
                    })
                })
            }
            PathSegment::Close => {
                out.bez.close_path();
                local_last = local_start;
                open = false;
                None
            }
        };
        out.arcs.push(arc);
    }
    close_if_returned(&mut out, start, open);
    out
}

/// Closes the open subpath at the end of `out` when its last point is back
/// at `start`.
fn close_if_returned(out: &mut MmPath, start: kurbo::Point, open: bool) {
    if !open {
        return;
    }
    let near = |p: &kurbo::Point| p.distance(start) <= SUBPATH_CLOSE_TOLERANCE_MM;
    let snapped = match out.bez.elements().last() {
        Some(PathEl::LineTo(p)) if near(p) => PathEl::LineTo(start),
        Some(PathEl::QuadTo(c, p)) if near(p) => PathEl::QuadTo(*c, start),
        Some(PathEl::CurveTo(c1, c2, p)) if near(p) => PathEl::CurveTo(*c1, *c2, start),
        _ => return,
    };
    // The snapped element keeps its arc mark: its sweep is unchanged.
    out.bez.pop();
    out.bez.push(snapped);
    out.bez.close_path();
    out.arcs.push(None);
}

/// One polyline per subpath of `path`: lines stay lines, arc pieces become
/// bulges, joined into arcs of up to 180°, and every other curve is
/// flattened to `tolerance_mm`. Otherwise the rule is
/// [`dc_geometry::flatten`]'s, which a path with no arcs gives exactly: a
/// subpath ending in `ClosePath` is closed, a point within [`EPS_MM`] of
/// the one before is dropped, and so is a subpath of fewer than two points.
fn build_profiles(path: &MmPath, tolerance_mm: f64) -> Vec<Polyline> {
    let mut out = Vec::new();
    let mut profile = ProfileBuilder::default();
    let mut current = kurbo::Point::ZERO;
    let mut subpath_start = current;
    for (el, arc) in path.bez.elements().iter().zip(&path.arcs) {
        match (*el, arc) {
            (PathEl::MoveTo(p), _) => {
                out.extend(std::mem::take(&mut profile).finish(false));
                profile.push(point(p), None);
                subpath_start = p;
            }
            (PathEl::LineTo(p), _) => profile.push(point(p), None),
            (PathEl::CurveTo(_, _, p), Some(hint)) => profile.arc_to(point(p), *hint),
            (PathEl::QuadTo(..) | PathEl::CurveTo(..), _) => {
                let mut curve = BezPath::new();
                curve.move_to(current);
                curve.push(*el);
                kurbo::flatten(&curve, tolerance_mm * KURBO_FLATTEN_SAFETY, |flat| {
                    if let PathEl::LineTo(p) = flat {
                        profile.push(point(p), None);
                    }
                });
            }
            (PathEl::ClosePath, _) => {
                out.extend(std::mem::take(&mut profile).finish(true));
            }
        }
        current = el.end_point().unwrap_or(subpath_start);
    }
    out.extend(profile.finish(false));
    out
}

fn point(p: kurbo::Point) -> Point {
    Point::new(p.x, p.y)
}

/// The points of one subpath so far and the segments between them.
#[derive(Default)]
struct ProfileBuilder {
    points: Vec<Point>,
    /// `arcs[i]` is the segment from `points[i]` to the next point: `None`
    /// for a line. One shorter than `points` until the profile is closed.
    arcs: Vec<Option<ArcPiece>>,
}

impl ProfileBuilder {
    /// Adds a segment to `end`, a line or `arc`, unless `end` is within
    /// [`EPS_MM`] of the last point. An arc that continues the previous one
    /// on the same circle is joined to it while the two sweep at most 180°.
    fn push(&mut self, end: Point, arc: Option<ArcPiece>) {
        let Some(last) = self.points.last_mut() else {
            self.points.push(end);
            return;
        };
        if last.distance_to(end) <= EPS_MM {
            return;
        }
        if let (Some(piece), Some(Some(previous))) = (arc, self.arcs.last_mut())
            && let Some(joined) = previous.joined(&piece)
        {
            *previous = joined;
            *last = end;
            return;
        }
        self.arcs.push(arc);
        self.points.push(end);
    }

    fn arc_to(&mut self, end: Point, hint: ArcHint) {
        let arc = self.points.last().map(|&start| ArcPiece {
            start,
            end,
            sweep_rad: hint.sweep_rad,
            tolerance_mm: hint.tolerance_mm,
        });
        self.push(end, arc);
    }

    /// The finished polyline, or `None` with fewer than two points. A
    /// closed subpath that goes once round one circle is the two
    /// half-circle arcs from its start, as `Polyline::circle` makes.
    fn finish(mut self, closed: bool) -> Option<Polyline> {
        if closed {
            match (self.points.first(), self.points.last()) {
                (Some(first), Some(last))
                    if self.points.len() > 1 && first.distance_to(*last) <= EPS_MM =>
                {
                    // The last segment now ends at the first point.
                    self.points.pop();
                }
                _ => self.arcs.push(None),
            }
        }
        let first = *self.points.first()?;
        if self.points.len() < 2 {
            return None;
        }
        if closed
            && let Some(pieces) = self.arcs.iter().copied().collect::<Option<Vec<_>>>()
            && let Some((centre, direction)) = arc::full_circle(&pieces)
        {
            let opposite = Point::new(2.0 * centre.x - first.x, 2.0 * centre.y - first.y);
            return Some(Polyline::new(
                vec![
                    Vertex::new(first.x, first.y, direction),
                    Vertex::new(opposite.x, opposite.y, direction),
                ],
                true,
            ));
        }
        let vertices = self
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let bulge = self
                    .arcs
                    .get(i)
                    .copied()
                    .flatten()
                    .map_or(0.0, |arc| arc.bulge());
                Vertex::new(p.x, p.y, bulge)
            })
            .collect();
        Some(Polyline::new(vertices, closed))
    }
}

/// Whether every point of `el` lies within [`MAX_COORDINATE_MM`] of the
/// origin on each axis. Besides matching what `.dcv` validation accepts,
/// the bound limits how many segments a curve can flatten into, so an
/// untrusted file cannot exhaust memory.
pub(crate) fn element_is_within_limit(el: &PathEl) -> bool {
    let within =
        |p: &kurbo::Point| p.x.abs() <= MAX_COORDINATE_MM && p.y.abs() <= MAX_COORDINATE_MM;
    match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => within(p),
        PathEl::QuadTo(c, p) => within(c) && within(p),
        PathEl::CurveTo(c1, c2, p) => within(c1) && within(c2) && within(p),
        PathEl::ClosePath => true,
    }
}

/// An upper estimate of the vertices `flatten` makes of `path`: Wang's
/// formula, the segment count that uniform subdivision needs to stay
/// within `tolerance_mm`. kurbo's adaptive flattening needs no more in
/// practice. Coordinates are within [`MAX_COORDINATE_MM`] and the tolerance
/// at least [`MIN_FLATTEN_TOLERANCE_MM`], so each curve's count is at most
/// a few tens of thousands.
pub(crate) fn estimated_vertices(path: &BezPath, tolerance_mm: f64) -> usize {
    let mut current = kurbo::Point::ZERO;
    let mut total: usize = 0;
    for el in path.elements() {
        let count = match *el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => {
                current = p;
                1
            }
            PathEl::QuadTo(c, p) => {
                let d = second_difference(current, c, p);
                current = p;
                wang_segments(0.25 * d, tolerance_mm)
            }
            PathEl::CurveTo(c1, c2, p) => {
                let d1 = second_difference(current, c1, c2);
                let d2 = second_difference(c1, c2, p);
                current = p;
                wang_segments(0.75 * d1.max(d2), tolerance_mm)
            }
            PathEl::ClosePath => 0,
        };
        total = total.saturating_add(count);
    }
    total
}

/// Length of the second difference `a − 2b + c` of three control points, mm.
fn second_difference(a: kurbo::Point, b: kurbo::Point, c: kurbo::Point) -> f64 {
    (a.to_vec2() - b.to_vec2() * 2.0 + c.to_vec2()).hypot()
}

/// `ceil(sqrt(scaled_second_difference / tolerance))`, at least 1.
fn wang_segments(scaled_second_difference_mm: f64, tolerance_mm: f64) -> usize {
    let segments = (scaled_second_difference_mm / tolerance_mm).sqrt().ceil();
    // Float-to-int `as` saturates and maps NaN to 0. With coordinates and
    // tolerance in their checked ranges the value is below 1e5.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "saturating cast of a bounded, non-negative count"
    )]
    let segments = segments as usize;
    segments.max(1)
}

pub(crate) fn element_is_finite(el: &PathEl) -> bool {
    let finite = |p: &kurbo::Point| p.x.is_finite() && p.y.is_finite();
    match el {
        PathEl::MoveTo(p) | PathEl::LineTo(p) => finite(p),
        PathEl::QuadTo(c, p) => finite(c) && finite(p),
        PathEl::CurveTo(c1, c2, p) => finite(c1) && finite(c2) && finite(p),
        PathEl::ClosePath => true,
    }
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
    use dc_geometry::{Polyline, Transform, assert_approx_eq};
    use dc_project::{DefinitionId, InstanceId};

    /// Coordinates pass through usvg as f32; at workpiece scale that is a
    /// few micrometres of rounding.
    const TOL_MM: f64 = 1e-4;

    fn import(bytes: &[u8]) -> (SvgImport, Vec<ImportWarning>) {
        import_svg(bytes, &SvgImportOptions::new()).expect("import succeeds")
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
        assert_approx_eq!(result.mm_per_user_unit, 1.0, 1e-6);
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
        assert_approx_eq!(result.mm_per_user_unit, 25.4 / 96.0, 1e-6);
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
        assert_approx_eq!(result.mm_per_user_unit, 0.254, 1e-6);
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
        assert_approx_eq!(result.mm_per_user_unit, 25.4 / 96.0, 1e-6);
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

    // ── arcs (#70) ───────────────────────────────────────────────────────

    /// Bulges an arc import computes from usvg's f32 control points, not
    /// snapped, agree with the exact value to this. Dimensionless.
    const BULGE_TOL: f64 = 1e-6;

    fn vertex_points(polyline: &Polyline) -> Vec<(f64, f64)> {
        polyline.vertices().iter().map(|v| (v.x, v.y)).collect()
    }

    fn assert_points(polyline: &Polyline, expected: &[(f64, f64)]) {
        let got = vertex_points(polyline);
        assert_eq!(got.len(), expected.len(), "vertices {got:?}");
        for (g, e) in got.iter().zip(expected) {
            assert_approx_eq!(g.0, e.0, TOL_MM);
            assert_approx_eq!(g.1, e.1, TOL_MM);
        }
    }

    fn assert_all_lines(polyline: &Polyline) {
        for (i, seg) in polyline.segments().enumerate() {
            assert!(seg.is_line(), "segment {i} is an arc: {seg:?}");
        }
    }

    /// A `<circle>` is two half-circle arcs, as `Polyline::circle` and a
    /// DXF `CIRCLE` are: the start point usvg uses, (cx + r, cy), and the
    /// point opposite. usvg draws circles towards +y in SVG's Y-down page,
    /// clockwise as seen; the Y flip keeps that clockwise, so both bulges
    /// are exactly −1.
    #[test]
    fn circle_imports_as_two_bulge_arcs() {
        let (result, warnings) = import(include_bytes!("../tests/fixtures/circle.svg"));
        assert!(warnings.is_empty(), "{warnings:?}");
        let profile = only_profile(&result);
        assert!(profile.is_closed());
        assert_points(profile, &[(95.0, 50.0), (5.0, 50.0)]);
        for v in profile.vertices() {
            assert_approx_eq!(v.bulge, -1.0, 0.0);
        }
        let (centre, radius_mm) = dc_geometry::as_circle(profile, EPS_MM).expect("a circle");
        assert_approx_eq!(centre.x, 50.0, TOL_MM);
        assert_approx_eq!(centre.y, 50.0, TOL_MM);
        assert_approx_eq!(radius_mm, 45.0, TOL_MM);
    }

    /// An `<ellipse>` with rx = ry is a circle.
    #[test]
    fn ellipse_with_equal_radii_imports_as_two_bulge_arcs() {
        let result = parse(
            r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100"><ellipse cx="40" cy="30" rx="12" ry="12"/></svg>"#,
        );
        let profile = only_profile(&result);
        assert_points(profile, &[(52.0, 70.0), (28.0, 70.0)]);
        for v in profile.vertices() {
            assert_approx_eq!(v.bulge, -1.0, 0.0);
        }
    }

    /// SVG `A` with rx = ry is one bulge segment per arc of up to 180°,
    /// though usvg hands it over as several Bézier pieces. The half circle
    /// from (10, 50) to (90, 50) with sweep-flag 1 runs through SVG (50, 10)
    /// (positive angles are clockwise on SVG's Y-down page), which is
    /// Y-up (50, 90): clockwise, bulge −1. The quarter from SVG (60, 20) to
    /// (70, 10), sweep-flag 0, is centred on SVG (60, 10), Y-up (60, 90),
    /// and runs counter-clockwise in Y-up: bulge +tan(22.5°).
    #[test]
    fn circular_arc_command_imports_as_bulge() {
        let result = parse(
            r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100">
                <path id="half" d="M 10 50 A 40 40 0 0 1 90 50" fill="none"/>
                <path id="quarter" d="M 60 20 A 10 10 0 0 0 70 10" fill="none"/>
            </svg>"#,
        );
        let half = &def(&result, "half").profiles;
        assert_eq!(half.len(), 1);
        assert!(!half[0].is_closed());
        assert_points(&half[0], &[(10.0, 50.0), (90.0, 50.0)]);
        assert_approx_eq!(half[0].vertices()[0].bulge, -1.0, BULGE_TOL);

        let quarter = &def(&result, "quarter").profiles;
        assert_eq!(quarter.len(), 1);
        assert_points(&quarter[0], &[(60.0, 80.0), (70.0, 90.0)]);
        let b = (std::f64::consts::FRAC_PI_8).tan();
        assert_approx_eq!(quarter[0].vertices()[0].bulge, b, BULGE_TOL);
        let arc = quarter[0]
            .segments()
            .next()
            .and_then(|s| s.arc())
            .expect("an arc");
        assert_approx_eq!(arc.centre.x, 60.0, TOL_MM);
        assert_approx_eq!(arc.centre.y, 90.0, TOL_MM);
        assert_approx_eq!(arc.radius_mm, 10.0, TOL_MM);
    }

    /// A rounded rectangle with rx = ry has circular corners, which usvg
    /// draws with the same arc command: four quarter arcs between four
    /// straight edges, eight vertices instead of a flattened polygon.
    #[test]
    fn rounded_rect_corners_import_as_quarter_arcs() {
        let result = parse(
            r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100"><rect x="10" y="10" width="40" height="20" rx="5" ry="5"/></svg>"#,
        );
        let profile = only_profile(&result);
        assert_eq!(profile.vertices().len(), 8);
        let b = (std::f64::consts::FRAC_PI_8).tan();
        let arcs: Vec<f64> = profile
            .vertices()
            .iter()
            .map(|v| v.bulge)
            .filter(|bulge| bulge.abs() > BULGE_TOL)
            .collect();
        assert_eq!(arcs.len(), 4, "bulges {:?}", profile.vertices());
        for bulge in arcs {
            assert_approx_eq!(bulge.abs(), b, BULGE_TOL);
        }
        assert_rect(
            bounds(&result.definitions[0].profiles),
            (10.0, 70.0),
            (50.0, 90.0),
        );
    }

    /// No bulge holds an ellipse, so one with rx ≠ ry is flattened.
    #[test]
    fn ellipse_with_unequal_radii_is_still_flattened() {
        let result = parse(
            r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100"><ellipse cx="50" cy="50" rx="40" ry="20"/></svg>"#,
        );
        let profile = only_profile(&result);
        assert!(profile.is_closed());
        assert!(profile.vertices().len() > 8);
        assert_all_lines(profile);
        assert_rect(
            bounds(&result.definitions[0].profiles),
            (10.0, 30.0),
            (90.0, 70.0),
        );
    }

    /// A circle under scale(2, 1) is an ellipse on the page: flattened.
    #[test]
    fn circle_under_non_uniform_scale_is_flattened() {
        let result = parse(
            r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100"><circle cx="20" cy="50" r="10" transform="scale(2 1)"/></svg>"#,
        );
        let profile = only_profile(&result);
        assert!(profile.vertices().len() > 8);
        assert_all_lines(profile);
        assert_rect(
            bounds(&result.definitions[0].profiles),
            (20.0, 40.0),
            (60.0, 60.0),
        );
    }

    /// Rotation and uniform scale keep a circle a circle: still two exact
    /// half-circle arcs, at the scaled radius.
    #[test]
    fn circle_under_rotation_and_uniform_scale_stays_two_arcs() {
        let result = parse(
            r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100"><g transform="translate(50 50) rotate(30) scale(2)"><circle cx="0" cy="0" r="10"/></g></svg>"#,
        );
        let profile = only_profile(&result);
        assert_eq!(profile.vertices().len(), 2);
        for v in profile.vertices() {
            assert_approx_eq!(v.bulge, -1.0, 0.0);
        }
        let (centre, radius_mm) = dc_geometry::as_circle(profile, EPS_MM).expect("a circle");
        assert_approx_eq!(centre.x, 50.0, TOL_MM);
        assert_approx_eq!(centre.y, 50.0, TOL_MM);
        assert_approx_eq!(radius_mm, 20.0, TOL_MM);
    }

    /// A reflection keeps a circle a circle but reverses its direction, so
    /// the mirrored circle's bulges are +1 where the plain one's are −1.
    /// scale(−1, 1) moves the centre from x −70 to 70.
    #[test]
    fn mirrored_circle_keeps_its_radius_and_flips_direction() {
        let result = parse(
            r#"<svg width="100mm" height="100mm" viewBox="0 0 100 100">
                <circle id="plain" cx="30" cy="50" r="10"/>
                <circle id="mirrored" cx="-70" cy="50" r="10" transform="scale(-1 1)"/>
            </svg>"#,
        );
        for (name, centre_x, bulge) in [("plain", 30.0, -1.0), ("mirrored", 70.0, 1.0)] {
            let profiles = &def(&result, name).profiles;
            assert_eq!(profiles.len(), 1, "{name}");
            assert_eq!(profiles[0].vertices().len(), 2, "{name}");
            for v in profiles[0].vertices() {
                assert_approx_eq!(v.bulge, bulge, 0.0);
            }
            let (centre, radius_mm) =
                dc_geometry::as_circle(&profiles[0], EPS_MM).expect("a circle");
            assert_approx_eq!(centre.x, centre_x, TOL_MM);
            assert_approx_eq!(centre.y, 50.0, TOL_MM);
            assert_approx_eq!(radius_mm, 10.0, TOL_MM);
        }
    }

    /// A cubic Bézier that is not a circular arc's is flattened, even when
    /// it bends like one: the plaque's squiggle.
    #[test]
    fn non_circular_cubic_is_still_flattened() {
        let result = parse(
            r#"<svg width="150mm" height="100mm" viewBox="0 0 150 100"><path d="M 95,88 C 105,65 120,100 130,75" fill="none"/></svg>"#,
        );
        let profile = only_profile(&result);
        assert!(profile.vertices().len() > 4);
        assert_all_lines(profile);
    }

    /// A path with no circular arc imports exactly as before #70, when
    /// `dc_geometry::flatten` flattened the whole path: per-curve
    /// flattening gives the same vertices. Lines, quadratics, free cubics,
    /// an ellipse, a stretched circle and a gap-closed subpath.
    #[test]
    fn paths_without_arcs_flatten_as_before() {
        let shapes = [
            r#"<path d="M 95,88 C 105,65 120,100 130,75 L 140,10 Q 120,5 100,20 Z M 5,5 L 20,5 L 5,20 L 5,5"/>"#,
            r#"<ellipse cx="50" cy="50" rx="40" ry="20"/>"#,
            r#"<circle cx="20" cy="50" r="10" transform="scale(2 1)"/>"#,
        ];
        for shape in shapes {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="150mm" height="100mm" viewBox="0 0 150 100">{shape}</svg>"#
            );
            let tree = usvg::Tree::from_str(&svg, &usvg_options()).expect("parses");
            fn first_path(group: &usvg::Group) -> Option<&usvg::Path> {
                group.children().iter().find_map(|node| match node {
                    usvg::Node::Path(p) => Some(p.as_ref()),
                    usvg::Node::Group(g) => first_path(g),
                    _ => None,
                })
            }
            let path = first_path(tree.root()).expect("a path");
            let px_to_mm = Affine::new([MM_PER_PX, 0.0, 0.0, -MM_PER_PX, 0.0, 100.0]);
            let mm_path = to_mm_path(path.data(), px_to_mm * affine(path.abs_transform()));
            assert!(mm_path.arcs.iter().all(Option::is_none), "{shape}");
            let tolerance_mm = FLATTEN_TOLERANCE_DEFAULT_MM;
            assert_eq!(
                build_profiles(&mm_path, tolerance_mm),
                dc_geometry::flatten(&mm_path.bez, tolerance_mm),
                "{shape}"
            );
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

    /// #99: the importer cannot know a feature's depth, so every instance
    /// leaves `z_top_mm` and `z_bottom_mm` unset.
    #[test]
    fn imported_instances_have_unset_depths() {
        let (result, _) = import_svg(
            include_bytes!("../tests/fixtures/y_flip.svg"),
            &SvgImportOptions::new(),
        )
        .unwrap();
        assert!(!result.instances.is_empty());
        assert_eq!(result.instances.len(), result.definitions.len());
        for (n, (inst, def)) in result.instances.iter().zip(&result.definitions).enumerate() {
            assert_eq!(inst.id, InstanceId::new(format!("i{}", n + 1)));
            assert_eq!(inst.definition, def.id);
            assert_eq!(def.id, DefinitionId::new(format!("d{}", n + 1)));
            assert_eq!(inst.name, def.name);
            assert_eq!(inst.transform, Transform::IDENTITY);
            assert_eq!(inst.z_top_mm, None);
            assert_eq!(inst.z_bottom_mm, None);
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
        let err = import_svg(b"<svg", &SvgImportOptions::new()).unwrap_err();
        assert!(matches!(err, ImportError::Svg(_)), "{err:?}");
    }

    #[test]
    fn non_positive_or_non_finite_options_are_rejected() {
        let bytes = include_bytes!("../tests/fixtures/mm_root.svg");
        for tolerance_mm in [0.0, -0.1, f64::NAN, f64::INFINITY] {
            let options = SvgImportOptions {
                flatten_tolerance_mm: tolerance_mm,
                ..SvgImportOptions::new()
            };
            assert!(matches!(
                import_svg(bytes, &options),
                Err(ImportError::InvalidTolerance { .. })
            ));
        }
    }

    // ── limits ───────────────────────────────────────────────────────────

    #[test]
    fn tolerance_below_minimum_is_rejected() {
        let bytes = include_bytes!("../tests/fixtures/mm_root.svg");
        let too_fine = SvgImportOptions {
            flatten_tolerance_mm: MIN_FLATTEN_TOLERANCE_MM / 2.0,
            ..SvgImportOptions::new()
        };
        assert!(matches!(
            import_svg(bytes, &too_fine),
            Err(ImportError::InvalidTolerance { .. })
        ));
        let finest = SvgImportOptions {
            flatten_tolerance_mm: MIN_FLATTEN_TOLERANCE_MM,
            ..SvgImportOptions::new()
        };
        assert!(import_svg(bytes, &finest).is_ok());
    }

    #[test]
    fn huge_coordinate_path_is_rejected() {
        // 2e8 user units at 1 mm each is 200 km, past MAX_COORDINATE_MM.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
            <rect id="ok" x="0" y="0" width="10" height="10"/>
            <path id="far" d="M0,0 C200000000,0 200000000,50 0,50" fill="none" stroke="black"/>
        </svg>"#;
        let err = import_svg(svg, &SvgImportOptions::new()).unwrap_err();
        match err {
            ImportError::CoordinateOutOfRange { path, limit_mm } => {
                assert_eq!(path, "far");
                assert_approx_eq!(limit_mm, MAX_COORDINATE_MM, 0.0);
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    /// #59: the import limit is the shared 6 m bound. A path just past it
    /// is refused; one just inside is imported.
    #[test]
    fn path_past_shared_coordinate_limit_is_rejected() {
        let svg = |x: f64| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
                <rect id="far" x="{x}" y="0" width="10" height="10"/>
            </svg>"#
            )
        };
        let limit = MAX_COORDINATE_MM;
        let options = SvgImportOptions::new();
        assert!(import_svg(svg(limit - 11.0).as_bytes(), &options).is_ok());
        match import_svg(svg(limit - 9.0).as_bytes(), &options).unwrap_err() {
            ImportError::CoordinateOutOfRange { path, limit_mm } => {
                assert_eq!(path, "far");
                assert_approx_eq!(limit_mm, limit, 0.0);
            }
            other => panic!("unexpected error {other:?}"),
        }
    }

    /// #59: the page side is bounded by the same limit.
    #[test]
    fn page_past_shared_coordinate_limit_is_rejected() {
        let svg = |width_mm: f64| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width_mm}mm" height="100mm" viewBox="0 0 100 100">
                <rect x="0" y="0" width="10" height="10"/>
            </svg>"#
            )
        };
        let limit = MAX_COORDINATE_MM;
        let options = SvgImportOptions::new();
        assert!(import_svg(svg(limit - 1.0).as_bytes(), &options).is_ok());
        assert!(matches!(
            import_svg(svg(limit + 1.0).as_bytes(), &options),
            Err(ImportError::InvalidPageSize { .. })
        ));
    }

    #[test]
    fn huge_page_is_rejected() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200000000mm" height="100mm" viewBox="0 0 100 100">
            <rect x="0" y="0" width="10" height="10"/>
        </svg>"#;
        assert!(matches!(
            import_svg(svg, &SvgImportOptions::new()),
            Err(ImportError::InvalidPageSize { .. })
        ));
    }

    #[test]
    fn vertex_cap_is_enforced_while_flattening() {
        // The ellipse flattens to far more than 10 vertices.
        let options = SvgImportOptions {
            max_vertices: 10,
            ..SvgImportOptions::new()
        };
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
            <ellipse cx="50" cy="50" rx="45" ry="30"/>
        </svg>"#;
        let err = import_svg(svg, &options).unwrap_err();
        assert!(
            matches!(err, ImportError::TooManyVertices { limit: 10 }),
            "{err:?}"
        );
    }

    #[test]
    fn vertex_cap_counts_across_paths() {
        // Two rectangles, four vertices each.
        let bytes = include_bytes!("../tests/fixtures/y_flip.svg");
        let capped = |max_vertices| SvgImportOptions {
            max_vertices,
            ..SvgImportOptions::new()
        };
        assert!(import_svg(bytes, &capped(8)).is_ok());
        assert!(matches!(
            import_svg(bytes, &capped(7)),
            Err(ImportError::TooManyVertices { limit: 7 })
        ));
    }

    #[test]
    fn default_options_use_the_documented_limits() {
        let options = SvgImportOptions::new();
        assert_eq!(options.max_vertices, MAX_IMPORT_VERTICES);
        const { assert!(MIN_FLATTEN_TOLERANCE_MM <= FLATTEN_TOLERANCE_DEFAULT_MM) };
    }

    #[test]
    fn non_finite_coordinates_are_rejected() {
        // A scale this large overflows f32 inside usvg or f64 after
        // conversion; either way nothing non-finite may come out.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">
            <g transform="scale(1e38)"><g transform="scale(1e38)"><rect width="10" height="10"/></g></g>
        </svg>"#;
        match import_svg(svg, &SvgImportOptions::new()) {
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
    // whatever the paint. Upstream asserts no coordinates, so the Y-up flip
    // changes no expected value here.

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
