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
mod round;
pub mod svg;

pub use dxf::{
    DxfImport, DxfImportOptions, DxfReadError, DxfUnits, JOIN_TOLERANCE_DEFAULT_MM,
    MAX_INSERT_DEPTH, import_dxf,
};
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
}

impl std::fmt::Display for ImportWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TextIgnored { count } => write!(
                f,
                "{count} text element(s) skipped; convert text to paths to import it"
            ),
            Self::ImagesIgnored { count } => write!(f, "{count} image(s) skipped"),
            Self::PaintServerIgnored { path } => write!(
                f,
                "path `{path}` uses a gradient or pattern; its outline was imported"
            ),
            Self::ClipPathIgnored { group } => write!(
                f,
                "clip path on group `{group}` ignored; its shapes were imported unclipped"
            ),
            Self::MaskIgnored { group } => write!(
                f,
                "mask on group `{group}` ignored; its shapes were imported unmasked"
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
        }
    }
}
