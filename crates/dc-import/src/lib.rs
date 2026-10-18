// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Importers for DreamCarve. Importers flip Y to the internal Y-up frame
//! (ADR-0001). Phase 1 adds SVG ([`import_svg`]), phase 2 DXF.
//!
//! Importers take bytes and return values: no file I/O, no printing.
//! Problems that leave a usable result are [`ImportWarning`]s; anything that
//! makes the result untrustworthy is an [`ImportError`].

pub mod svg;

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
    /// The stock thickness is not a positive finite length.
    #[error("stock thickness {thickness_mm} mm must be a positive finite length")]
    InvalidThickness {
        /// The rejected thickness, millimetres.
        thickness_mm: f64,
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
        }
    }
}
