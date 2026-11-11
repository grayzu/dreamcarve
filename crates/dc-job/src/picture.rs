// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pictures placed in a project (`docs/specs/phase-3/design.md`, "CLI";
//! functional spec, capability 46): a decoded picture
//! ([`dc_import::bitmap::import`]) made a [`Bitmap`] at a size and a
//! corner, appended to a project with the next free id, and listed. `init`
//! on a picture is [`crate::project_from::picture`]; `image add` and the
//! app's *Import picture* (#544) are [`picture_into`]; `image list` is
//! [`picture_rows`]. The shell reads the file and decodes it; nothing here
//! does I/O.

use dc_geometry::{MAX_COORDINATE_MM, Point};
use dc_project::{
    Bitmap, BitmapId, Grayscale, LoadError, Operation, OperationId, Project, round_length_mm,
};

use crate::error::JobError;
use crate::import::next_free;

/// How big a picture is on the stock: one side, the other following the
/// pixels' proportions, so the picture is never stretched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PictureSize {
    /// The footprint's width, mm.
    Width {
        /// The width, mm.
        width_mm: f64,
    },
    /// The footprint's height, mm.
    Height {
        /// The height, mm.
        height_mm: f64,
    },
}

impl PictureSize {
    /// The footprint, `(width_mm, height_mm)`, of a `width_px × height_px`
    /// picture at this size: the side given, and the other in the pixels'
    /// proportion, each rounded to [`dc_project::LENGTH_DECIMALS`] places
    /// (a micrometre's thousandth), well within validation's 1e-6 on the
    /// proportions for any footprint the stock can hold.
    ///
    /// # Errors
    ///
    /// [`JobError::PictureSize`] when the side given is not a positive,
    /// finite length, or either side rounds to 0.
    pub fn footprint_mm(self, width_px: u32, height_px: u32) -> Result<(f64, f64), JobError> {
        let given_mm = match self {
            Self::Width { width_mm } => width_mm,
            Self::Height { height_mm } => height_mm,
        };
        if !(given_mm.is_finite() && given_mm > 0.0) {
            return Err(JobError::PictureSize { size_mm: given_mm });
        }
        let (w, h) = (f64::from(width_px), f64::from(height_px));
        let (width_mm, height_mm) = match self {
            Self::Width { width_mm } => (width_mm, width_mm * h / w),
            Self::Height { height_mm } => (height_mm * w / h, height_mm),
        };
        let footprint = (round_length_mm(width_mm), round_length_mm(height_mm));
        if !(footprint.0 > 0.0
            && footprint.1 > 0.0
            && footprint.0.is_finite()
            && footprint.1.is_finite())
        {
            return Err(JobError::PictureSize { size_mm: given_mm });
        }
        Ok(footprint)
    }
}

/// The bitmap `id`, named `name`, holding `pixels` (stored as
/// [`dc_import::bitmap::stored`] writes them) at `size` with its
/// bottom-left corner at `corner`, project mm, Y-up. Visible and
/// unlocked.
///
/// # Errors
///
/// [`JobError::PictureSize`] for a size that is not a positive length,
/// [`JobError::PictureOutOfRange`] for a footprint reaching past
/// [`MAX_COORDINATE_MM`], and [`JobError::Picture`] when the pixels cannot
/// be encoded.
pub fn bitmap(
    id: BitmapId,
    name: &str,
    pixels: &Grayscale,
    size: PictureSize,
    corner: Point,
) -> Result<Bitmap, JobError> {
    let (width_mm, height_mm) = size.footprint_mm(pixels.width_px, pixels.height_px)?;
    let within = |v: f64| v.is_finite() && v.abs() <= MAX_COORDINATE_MM;
    if ![
        corner.x,
        corner.y,
        corner.x + width_mm,
        corner.y + height_mm,
    ]
    .into_iter()
    .all(within)
    {
        return Err(JobError::PictureOutOfRange {
            x_mm: corner.x,
            y_mm: corner.y,
            width_mm,
            height_mm,
            limit_mm: MAX_COORDINATE_MM,
        });
    }
    Ok(Bitmap {
        id,
        name: name.to_owned(),
        width_px: pixels.width_px,
        height_px: pixels.height_px,
        x_mm: corner.x,
        y_mm: corner.y,
        width_mm,
        height_mm,
        visible: true,
        locked: false,
        png_base64: dc_import::bitmap::stored(pixels)?,
    })
}

/// `project` with the picture `pixels` appended as a new bitmap: the next
/// free `b<n>` id after the project's own, named `name`, at `size` with
/// its bottom-left corner at `corner` (functional spec, capability 46;
/// `image add`). The rest of the project is unchanged. Returns the new
/// bitmap's id.
///
/// # Errors
///
/// As [`bitmap`].
pub fn picture_into(
    project: &Project,
    name: &str,
    pixels: &Grayscale,
    size: PictureSize,
    corner: Point,
) -> Result<(Project, BitmapId), JobError> {
    let n = next_free(project.bitmaps.iter().map(|b| b.id.as_str()), 'b');
    let id = BitmapId::new(format!("b{n}"));
    let bitmap = bitmap(id.clone(), name, pixels, size, corner)?;
    let mut project = project.clone();
    project.bitmaps.push(bitmap);
    Ok((project, id))
}

/// One row of `image list`: a bitmap as the file says it, its pixel size
/// from its PNG's header.
#[derive(Debug, Clone, PartialEq)]
pub struct PictureRow {
    /// The bitmap's id.
    pub id: BitmapId,
    /// The bitmap's name.
    pub name: String,
    /// The picture's width, pixels, from its PNG's header.
    pub width_px: u32,
    /// The picture's height, pixels, from its PNG's header.
    pub height_px: u32,
    /// X of the footprint's bottom-left corner, mm.
    pub x_mm: f64,
    /// Y of the footprint's bottom-left corner, mm.
    pub y_mm: f64,
    /// The footprint's width, mm.
    pub width_mm: f64,
    /// The footprint's height, mm.
    pub height_mm: f64,
    /// The Photo V-Carve operations that name it, in project order.
    pub used_by: Vec<OperationId>,
}

/// The rows of `image list`, in project order. Each reads its bitmap's
/// PNG header ([`Bitmap::header`]), never its pixels, so listing costs
/// nothing per pixel.
///
/// # Errors
///
/// [`LoadError::Bitmap`] for a bitmap whose stored picture has no
/// readable header (a project `load` would have refused).
pub fn picture_rows(project: &Project) -> Result<Vec<PictureRow>, LoadError> {
    project
        .bitmaps
        .iter()
        .map(|bitmap| {
            let header = bitmap.header()?;
            let used_by = project
                .operations
                .iter()
                .filter_map(|op| match op {
                    Operation::PhotoVCarve(photo) if photo.image == bitmap.id => {
                        Some(photo.id.clone())
                    }
                    _ => None,
                })
                .collect();
            Ok(PictureRow {
                id: bitmap.id.clone(),
                name: bitmap.name.clone(),
                width_px: header.width_px,
                height_px: header.height_px,
                x_mm: bitmap.x_mm,
                y_mm: bitmap.y_mm,
                width_mm: bitmap.width_mm,
                height_mm: bitmap.height_mm,
                used_by,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The other side follows the pixels' proportions, whichever is given.
    #[test]
    fn footprint_follows_the_pixels() {
        let width = PictureSize::Width { width_mm: 100.0 };
        let height = PictureSize::Height { height_mm: 50.0 };
        assert_eq!(width.footprint_mm(64, 32).unwrap(), (100.0, 50.0));
        assert_eq!(height.footprint_mm(64, 32).unwrap(), (100.0, 50.0));
        assert_eq!(width.footprint_mm(3, 7).unwrap(), (100.0, 233.333_333));
        assert!(matches!(
            PictureSize::Width { width_mm: 1e-9 }.footprint_mm(1, 1),
            Err(JobError::PictureSize { .. })
        ));
    }
}
