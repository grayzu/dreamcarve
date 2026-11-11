// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Importers for DreamCarve. Importers bring geometry into the internal
//! Y-up frame (ADR-0001): SVG ([`import_svg`], phase 1) flips Y; DXF
//! ([`import_dxf`], phase 2) is already Y-up and only scales to millimetres.
//!
//! Importers take bytes and return values: no file I/O, no printing.
//! Problems that leave a usable result are [`ImportWarning`]s; anything that
//! makes the result untrustworthy is an [`ImportError`].

pub mod dxf;
pub mod prep;
mod round;
pub mod svg;

pub use dxf::{
    DxfImport, DxfImportOptions, DxfLayer, DxfReadError, DxfUnits, JOIN_TOLERANCE_DEFAULT_MM,
    MAX_INSERT_DEPTH, import_dxf,
};
pub use prep::walk::HiddenReason;
pub use svg::{
    MAX_IMPORT_VERTICES, MIN_FLATTEN_TOLERANCE_MM, RootUnits, SvgImport, SvgImportOptions,
    import_svg,
};

/// Why an import produced no result.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    /// The bytes are not an SVG document usvg can read (not UTF-8, not
    /// well-formed XML, compressed SVGZ, or no valid size).
    #[error("SVG could not be read: {0}")]
    Svg(#[from] usvg::Error),
    /// The bytes are not a readable DXF document (a group code or value
    /// that does not parse, or a truncated file).
    #[error("DXF could not be read: {0}")]
    Dxf(#[from] DxfReadError),
    /// The DXF declares no units DreamCarve can scale from (`$INSUNITS`
    /// absent, 0, or a unit outside inch, foot, mile, mm, cm, m) and the
    /// caller gave no override.
    #[error(
        "DXF declares no usable units ($INSUNITS is {declared}); say which units it is drawn in"
    )]
    UnitsUnknown {
        /// What `$INSUNITS` held: `unitless` when absent or 0, else the
        /// unit's name.
        declared: String,
    },
    /// The join tolerance is negative or not finite.
    #[error("join tolerance {tolerance_mm} mm must be a finite length of 0 or more")]
    InvalidJoinTolerance {
        /// The rejected tolerance, millimetres.
        tolerance_mm: f64,
    },
    /// A path's coordinates are NaN or infinite after transforming to mm.
    #[error("path `{path}` has a coordinate that is not a finite number")]
    NonFinite {
        /// Name the path would have been imported under.
        path: String,
    },
    /// A path reaches further from the origin than
    /// [`dc_geometry::MAX_COORDINATE_MM`] once converted to millimetres.
    #[error("path `{path}` reaches beyond {limit_mm} mm from the origin")]
    CoordinateOutOfRange {
        /// Name the path would have been imported under.
        path: String,
        /// The limit it exceeded, millimetres.
        limit_mm: f64,
    },
    /// Flattening would produce more than `limit` vertices in total.
    #[error(
        "the drawing flattens to more than {limit} vertices; simplify it or raise the tolerance"
    )]
    TooManyVertices {
        /// The vertex cap that was exceeded.
        limit: usize,
    },
    /// Expanding the drawing's block inserts and arrays (INSERT, MINSERT)
    /// would visit more than `limit` entities, almost always nested arrays
    /// of blocks that hold little or nothing.
    #[error(
        "the drawing's block inserts and arrays expand to more than {limit} entities; explode or simplify them"
    )]
    TooMuchWork {
        /// The entity-visit cap that was exceeded.
        limit: usize,
    },
    /// The page size is NaN, infinite, not positive, or larger than
    /// [`dc_geometry::MAX_COORDINATE_MM`] on a side.
    #[error(
        "SVG page size {width_mm} × {height_mm} mm is not a positive finite size within limits"
    )]
    InvalidPageSize {
        /// Page width, millimetres.
        width_mm: f64,
        /// Page height, millimetres.
        height_mm: f64,
    },
    /// The flattening tolerance is not finite or is below
    /// [`svg::MIN_FLATTEN_TOLERANCE_MM`].
    #[error("flatten tolerance {tolerance_mm} mm must be a finite length of at least 0.001 mm")]
    InvalidTolerance {
        /// The rejected tolerance, millimetres.
        tolerance_mm: f64,
    },
}

/// Something in the input that was skipped or reinterpreted. The import
/// result is still usable. Path names are the definition names the paths
/// were (or would have been) imported under.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportWarning {
    /// `count` text elements were skipped; text is out of scope in phase 1.
    /// Convert text to paths in the drawing program to import it.
    TextIgnored {
        /// Number of `<text>` elements in the document.
        count: usize,
    },
    /// `count` raster or nested-SVG images were skipped.
    ImagesIgnored {
        /// Number of `<image>` elements in the document.
        count: usize,
    },
    /// Path `path` is painted with a gradient or pattern. Its geometry was
    /// imported; paint never affects geometry.
    PaintServerIgnored {
        /// Name of the imported definition.
        path: String,
    },
    /// The group `group` has a clip path. Its contents were imported
    /// unclipped.
    ClipPathIgnored {
        /// The group's SVG id, or empty when it has none.
        group: String,
    },
    /// The group `group` has a mask. Its contents were imported unmasked.
    MaskIgnored {
        /// The group's SVG id, or empty when it has none.
        group: String,
    },
    /// Closed subpath `subpath` (0-based, in path order) of `path` encloses
    /// no area and was dropped.
    ZeroAreaDropped {
        /// Name of the path.
        path: String,
        /// Index of the subpath among the path's flattened subpaths.
        subpath: usize,
    },
    /// Path `path` produced no usable geometry and no definition was made.
    EmptyPath {
        /// Name of the path.
        path: String,
    },
    /// The root declares a physical size in `units` but has no `viewBox`.
    /// Per the SVG specification, user units are then CSS pixels (96 per
    /// inch), not `units`; check the imported size.
    PhysicalSizeWithoutViewBox {
        /// The units on the root `width`/`height`.
        units: RootUnits,
    },
    /// DXF entities DreamCarve does not read were skipped: how many of each
    /// type, keyed by the DXF type name (`TEXT`, `DIMENSION`, `HATCH`, ...).
    /// `POLYLINE (mesh)` counts polygon and polyface meshes; `<TYPE>
    /// (degenerate)` counts entities with no length, too few vertices, an
    /// INSERT scale of 0, or a zero extrusion direction; `<TYPE> (not in
    /// the XY plane)` counts entities whose extrusion direction is not ±Z.
    DxfEntitiesSkipped {
        /// Skipped entities per type name, in name order.
        counts: std::collections::BTreeMap<String, usize>,
    },
    /// An INSERT of block `block` nests more than [`MAX_INSERT_DEPTH`]
    /// levels deep; the levels past the limit were skipped.
    DxfInsertTooDeep {
        /// The block whose insert exceeded the limit.
        block: String,
    },
    /// An INSERT names a block the file does not define; it was skipped.
    DxfBlockMissing {
        /// The missing block's name.
        block: String,
    },
    /// A DXF from before R2007 is written in a code page DreamCarve has
    /// no table for, such as the multi-byte `ANSI_932` (Shift-JIS), and
    /// some of its text is not ASCII. That text, such as layer names and
    /// so shape names, was read as Windows-1252 and may show wrong
    /// characters; the geometry is not affected.
    DxfCodePageUnsupported {
        /// The `$DWGCODEPAGE` value, as the file writes it.
        code_page: String,
    },
    /// Outlines of different paths cross (3.4, `shapes_overlap`): `count`
    /// pairs, the first named, and where its two shapes meet.
    ShapesOverlap {
        /// Number of overlapping pairs of paths.
        count: usize,
        /// Name of the earlier path of the first pair.
        first: String,
        /// Name of the later path of the first pair.
        second: String,
        /// X of the middle of the first pair's crossing, mm, Y-up frame.
        x_mm: f64,
        /// Y of the middle of the first pair's crossing, mm, Y-up frame.
        y_mm: f64,
    },
    /// A filled path lies wholly inside another filled path (3.4,
    /// `svg_hole_as_separate_path`): `count` such inner paths.
    SvgHoleAsSeparatePath {
        /// Number of inner paths.
        count: usize,
        /// Name of the first inner path.
        inner: String,
        /// Name of the path that contains it.
        outer: String,
    },
    /// A path has a stroke and no fill (3.4, `svg_stroke_only_path`), so
    /// the importer reads its centreline, an open path.
    SvgStrokeOnlyPath {
        /// Number of such paths.
        count: usize,
        /// Name of the first.
        first: String,
        /// The first one's stroke width, mm.
        width_mm: f64,
    },
    /// An object that is not drawn (3.4, `svg_hidden_object`).
    SvgHiddenObject {
        /// Number of hidden shapes.
        count: usize,
        /// The first one's id, or its layer's.
        first: String,
        /// Why the first is not drawn.
        reason: HiddenReason,
    },
    /// Text, images, clip paths or masks that cannot become shapes (3.4,
    /// `svg_object_not_convertible`), counted over the whole document.
    SvgObjectNotConvertible {
        /// Which kind.
        kind: NotConvertible,
        /// How many there are.
        count: usize,
        /// For a clip path or mask, the id of the first group using it,
        /// or empty; empty for text and images.
        group: String,
    },
    /// The stroker could not build the outline of a path's stroke (3.4,
    /// `prep_stroke_failed`), so the path's stroke was left out of the
    /// prepared drawing.
    PrepStrokeFailed {
        /// Name of the path.
        path: String,
    },
    /// Pieces or holes whose widest point is under the default minimum
    /// detail (3.4, `feature_below_size`).
    FeatureBelowSize {
        /// Number of pieces and holes under the threshold.
        count: usize,
        /// The smallest one's widest point, mm.
        smallest_mm: f64,
        /// The threshold, mm.
        threshold_mm: f64,
        /// The design's height the threshold is relative to, mm.
        height_mm: f64,
    },
}

/// The kinds of SVG content that cannot become shapes
/// ([`ImportWarning::SvgObjectNotConvertible`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotConvertible {
    /// Live `<text>`.
    Text,
    /// A raster or nested-SVG `<image>`.
    Image,
    /// A clip path.
    ClipPath,
    /// A mask.
    Mask,
}

impl NotConvertible {
    /// The kind as the warning table writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::ClipPath => "clip_path",
            Self::Mask => "mask",
        }
    }
}

impl ImportWarning {
    /// The stable `snake_case` code of the warning (functional 49g). The
    /// four existing text, image, clip-path and mask warnings share
    /// `svg_object_not_convertible` with the 3.4 variant that counts them.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::TextIgnored { .. }
            | Self::ImagesIgnored { .. }
            | Self::ClipPathIgnored { .. }
            | Self::MaskIgnored { .. }
            | Self::SvgObjectNotConvertible { .. } => "svg_object_not_convertible",
            Self::PaintServerIgnored { .. } => "svg_paint_server_ignored",
            Self::ZeroAreaDropped { .. } => "svg_zero_area_dropped",
            Self::EmptyPath { .. } => "svg_empty_path",
            Self::PhysicalSizeWithoutViewBox { .. } => "svg_physical_size_without_view_box",
            Self::DxfEntitiesSkipped { .. } => "dxf_entities_skipped",
            Self::DxfInsertTooDeep { .. } => "dxf_insert_too_deep",
            Self::DxfBlockMissing { .. } => "dxf_block_missing",
            Self::DxfCodePageUnsupported { .. } => "dxf_code_page_unsupported",
            Self::ShapesOverlap { .. } => "shapes_overlap",
            Self::SvgHoleAsSeparatePath { .. } => "svg_hole_as_separate_path",
            Self::SvgStrokeOnlyPath { .. } => "svg_stroke_only_path",
            Self::SvgHiddenObject { .. } => "svg_hidden_object",
            Self::PrepStrokeFailed { .. } => "prep_stroke_failed",
            Self::FeatureBelowSize { .. } => "feature_below_size",
        }
    }
}

/// Appended to the messages that `image-prep` fixes.
const IMAGE_PREP_FIXES: &str = "; `dreamcarve image-prep` would fix this";

impl std::fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextIgnored { count } => write!(
                f,
                "{count} text element(s) skipped; convert text to paths in your drawing program to import it"
            ),
            Self::ImagesIgnored { count } => {
                write!(
                    f,
                    "{count} image(s) skipped; trace images in Inkscape first"
                )
            }
            Self::PaintServerIgnored { path } => write!(
                f,
                "path `{path}` uses a gradient or pattern; its outline was imported"
            ),
            Self::ClipPathIgnored { group } => write!(
                f,
                "clip path on group `{group}` ignored; its shapes were imported unclipped; expand clips and masks in your drawing program"
            ),
            Self::MaskIgnored { group } => write!(
                f,
                "mask on group `{group}` ignored; its shapes were imported unmasked; expand clips and masks in your drawing program"
            ),
            Self::ZeroAreaDropped { path, subpath } => write!(
                f,
                "closed subpath {subpath} of path `{path}` encloses no area and was dropped"
            ),
            Self::EmptyPath { path } => {
                write!(f, "path `{path}` has no usable geometry and was skipped")
            }
            Self::PhysicalSizeWithoutViewBox { units } => write!(
                f,
                "page size is in {units} but there is no viewBox, so coordinates are read as CSS pixels (96 per inch)"
            ),
            Self::DxfEntitiesSkipped { counts } => {
                let total: usize = counts.values().sum();
                write!(
                    f,
                    "dxf_entities_skipped: {total} entity(ies) DreamCarve does not read were skipped: "
                )?;
                for (i, (kind, count)) in counts.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{kind}: {count}")?;
                }
                Ok(())
            }
            Self::DxfInsertTooDeep { block } => write!(
                f,
                "block `{block}` is inserted more than {MAX_INSERT_DEPTH} levels deep; the deeper levels were skipped"
            ),
            Self::DxfBlockMissing { block } => {
                write!(f, "INSERT of undefined block `{block}` was skipped")
            }
            Self::DxfCodePageUnsupported { code_page } => write!(
                f,
                "the drawing's text is in code page `{code_page}`, which DreamCarve cannot read; layer names were read as Windows-1252 and may show wrong characters"
            ),
            Self::ShapesOverlap {
                count,
                first,
                second,
                x_mm,
                y_mm,
            } => write!(
                f,
                "{count} pair(s) of shapes overlap; the first is `{first}` and `{second}`, meeting near ({x_mm:.2}, {y_mm:.2}) mm; unite them or move them apart{IMAGE_PREP_FIXES}"
            ),
            Self::SvgHoleAsSeparatePath {
                count,
                inner,
                outer,
            } => write!(
                f,
                "{count} filled path(s) lie wholly inside another filled path; the first is `{inner}` inside `{outer}`. They are separate shapes, a hole only where an operation targets both; make each one path with a hole{IMAGE_PREP_FIXES}"
            ),
            Self::SvgStrokeOnlyPath {
                count,
                first,
                width_mm,
            } => write!(
                f,
                "{count} path(s) have a stroke and no fill; the first is `{first}`, stroked {width_mm:.3} mm wide. Only its centreline is imported, an open path no Inlay or V-Carve can cut; turn strokes into shapes{IMAGE_PREP_FIXES}"
            ),
            Self::SvgHiddenObject {
                count,
                first,
                reason,
            } => write!(
                f,
                "{count} object(s) are not drawn ({}) but are imported and would be cut; the first is `{first}`; skip them{IMAGE_PREP_FIXES}",
                reason.describe()
            ),
            Self::SvgObjectNotConvertible { kind, count, group } => {
                let on = if group.is_empty() {
                    String::new()
                } else {
                    format!(", the first on `{group}`")
                };
                match kind {
                    NotConvertible::Text => write!(
                        f,
                        "{count} text element(s) skipped; convert text to paths in your drawing program"
                    ),
                    NotConvertible::Image => write!(
                        f,
                        "{count} image(s) skipped; trace images in Inkscape first"
                    ),
                    NotConvertible::ClipPath => write!(
                        f,
                        "{count} clip path(s) ignored{on}; their shapes are imported unclipped; expand clips and masks in your drawing program"
                    ),
                    NotConvertible::Mask => write!(
                        f,
                        "{count} mask(s) ignored{on}; their shapes are imported unmasked; expand clips and masks in your drawing program"
                    ),
                }
            }
            Self::PrepStrokeFailed { path } => write!(
                f,
                "the stroke of {path} could not be turned into an area and was left out"
            ),
            Self::FeatureBelowSize {
                count,
                smallest_mm,
                threshold_mm,
                height_mm,
            } => write!(
                f,
                "{count} piece(s) or hole(s) are narrower than {threshold_mm:.3} mm at their widest, 0.2% of the design's {height_mm:.1} mm height; the smallest is {smallest_mm:.3} mm. Specks that small are not worth carving{IMAGE_PREP_FIXES}"
            ),
        }
    }
}

#[cfg(test)]
mod warning {
    use super::*;

    /// Every variant, the existing ones included, has a stable code
    /// (functional 49g).
    #[test]
    fn existing_import_warnings_have_codes() {
        let cases = [
            (
                ImportWarning::TextIgnored { count: 1 },
                "svg_object_not_convertible",
            ),
            (
                ImportWarning::ImagesIgnored { count: 1 },
                "svg_object_not_convertible",
            ),
            (
                ImportWarning::ClipPathIgnored { group: "g".into() },
                "svg_object_not_convertible",
            ),
            (
                ImportWarning::MaskIgnored { group: "g".into() },
                "svg_object_not_convertible",
            ),
            (
                ImportWarning::PaintServerIgnored { path: "p".into() },
                "svg_paint_server_ignored",
            ),
            (
                ImportWarning::ZeroAreaDropped {
                    path: "p".into(),
                    subpath: 0,
                },
                "svg_zero_area_dropped",
            ),
            (
                ImportWarning::EmptyPath { path: "p".into() },
                "svg_empty_path",
            ),
            (
                ImportWarning::PhysicalSizeWithoutViewBox {
                    units: RootUnits::Millimetres,
                },
                "svg_physical_size_without_view_box",
            ),
            (
                ImportWarning::DxfEntitiesSkipped {
                    counts: std::collections::BTreeMap::new(),
                },
                "dxf_entities_skipped",
            ),
            (
                ImportWarning::DxfInsertTooDeep { block: "b".into() },
                "dxf_insert_too_deep",
            ),
            (
                ImportWarning::DxfBlockMissing { block: "b".into() },
                "dxf_block_missing",
            ),
            (
                ImportWarning::DxfCodePageUnsupported {
                    code_page: "ANSI_932".into(),
                },
                "dxf_code_page_unsupported",
            ),
            (
                ImportWarning::PrepStrokeFailed { path: "p".into() },
                "prep_stroke_failed",
            ),
        ];
        for (warning, code) in cases {
            assert_eq!(warning.code(), code, "{warning}");
        }
    }

    /// The fixes the existing messages gained.
    #[test]
    fn existing_messages_name_their_fix() {
        let text = ImportWarning::TextIgnored { count: 2 }.to_string();
        assert!(text.contains("convert text to paths in your drawing program"));
        let image = ImportWarning::ImagesIgnored { count: 1 }.to_string();
        assert!(image.contains("trace images in Inkscape first"));
        let clip = ImportWarning::ClipPathIgnored { group: "g".into() }.to_string();
        assert!(clip.contains("expand clips and masks"));
        let mask = ImportWarning::MaskIgnored { group: "g".into() }.to_string();
        assert!(mask.contains("expand clips and masks"));
    }
}
