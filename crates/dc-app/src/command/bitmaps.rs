// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The commands on a project's bitmaps (functional spec, capabilities 56
//! and 57): a picture imported as `dreamcarve image add` adds it, and a
//! Pictures row's eye, padlock, name and *Delete*; and a picture moved as
//! a shape is (slice 15, #544; capability 58).

use dc_geometry::{MAX_COORDINATE_MM, approx_eq};
use dc_job::JobError;
use dc_job::picture::{PictureSize, picture_into};
use dc_project::{Bitmap, BitmapId, Grayscale, Project, round_length_mm};

use crate::error::CommandError;
use crate::import::{PICTURE_CORNER, stem};
use crate::strings::{StrKey, Strings};

/// A move shorter than this on both axes, mm, moves nothing.
const NO_MOVE_MM: f64 = 1e-9;

/// `project` with the picture `pixels`, from the file `name`, appended as
/// its next bitmap: named by the file's stem, at `size`, its bottom-left
/// corner at the stock's ([`dc_job::picture::picture_into`], what `image
/// add` runs).
pub(super) fn import(
    project: &Project,
    name: &str,
    pixels: &Grayscale,
    size: PictureSize,
) -> Result<Project, CommandError> {
    Ok(picture_into(project, stem(name), pixels, size, PICTURE_CORNER)?.0)
}

/// The bitmap `id` of `project`.
fn find<'a>(project: &'a Project, id: &BitmapId) -> Result<&'a Bitmap, CommandError> {
    project
        .bitmaps
        .iter()
        .find(|b| b.id == *id)
        .ok_or_else(|| CommandError::UnknownBitmap { id: id.clone() })
}

/// Refuses to move or resize `ids` when any is locked, naming the locked
/// ones in selection order (capability 58: a lock refuses every
/// transform, and the status bar says why).
fn refuse_locked(project: &Project, ids: &[BitmapId]) -> Result<(), CommandError> {
    let names: Vec<String> = ids
        .iter()
        .filter_map(|id| project.bitmaps.iter().find(|b| b.id == *id))
        .filter(|b| b.locked)
        .map(|b| b.name.clone())
        .collect();
    if names.is_empty() {
        Ok(())
    } else {
        Err(CommandError::PictureLocked { names })
    }
}

/// Refuses a footprint `width_mm × height_mm` with its bottom-left corner
/// at (`x_mm`, `y_mm`) that reaches past the coordinate limit, as
/// [`dc_job::picture::footprint_at`] refuses one at import.
fn in_range(x_mm: f64, y_mm: f64, width_mm: f64, height_mm: f64) -> Result<(), CommandError> {
    let within = |v: f64| v.is_finite() && v.abs() <= MAX_COORDINATE_MM;
    if [x_mm, y_mm, x_mm + width_mm, y_mm + height_mm]
        .into_iter()
        .all(within)
    {
        Ok(())
    } else {
        Err(JobError::PictureOutOfRange {
            x_mm,
            y_mm,
            width_mm,
            height_mm,
            limit_mm: MAX_COORDINATE_MM,
        }
        .into())
    }
}

/// Where a bitmap at (`x_mm`, `y_mm`) is after a move by (`dx_mm`,
/// `dy_mm`): its corner, rounded as a stored length is.
fn moved(x_mm: f64, y_mm: f64, dx_mm: f64, dy_mm: f64) -> (f64, f64) {
    (round_length_mm(x_mm + dx_mm), round_length_mm(y_mm + dy_mm))
}

/// Whether `MoveBitmaps` can move `ids` by (`dx_mm`, `dy_mm`): some are
/// named, the project has each, none is locked, there is a distance to
/// go, and each footprint stays within the coordinate limit.
pub(crate) fn check_move(
    project: &Project,
    ids: &[BitmapId],
    dx_mm: f64,
    dy_mm: f64,
) -> Result<(), CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    super::finite("bitmaps[].x_mm", dx_mm)?;
    super::finite("bitmaps[].y_mm", dy_mm)?;
    for id in ids {
        find(project, id)?;
    }
    refuse_locked(project, ids)?;
    if approx_eq(dx_mm, 0.0, NO_MOVE_MM) && approx_eq(dy_mm, 0.0, NO_MOVE_MM) {
        return Err(CommandError::NothingToDo);
    }
    for bitmap in project.bitmaps.iter().filter(|b| ids.contains(&b.id)) {
        let (x_mm, y_mm) = moved(bitmap.x_mm, bitmap.y_mm, dx_mm, dy_mm);
        in_range(x_mm, y_mm, bitmap.width_mm, bitmap.height_mm)?;
    }
    Ok(())
}

/// `MoveBitmaps`: each of `ids` moved by (`dx_mm`, `dy_mm`). Nothing
/// moves unless every one can ([`check_move`]).
pub(super) fn move_by(
    project: &mut Project,
    ids: &[BitmapId],
    dx_mm: f64,
    dy_mm: f64,
) -> Result<(), CommandError> {
    check_move(project, ids, dx_mm, dy_mm)?;
    for bitmap in project.bitmaps.iter_mut().filter(|b| ids.contains(&b.id)) {
        (bitmap.x_mm, bitmap.y_mm) = moved(bitmap.x_mm, bitmap.y_mm, dx_mm, dy_mm);
    }
    Ok(())
}

/// The bitmap `id` of `project`, to change.
pub(super) fn find_mut<'a>(
    project: &'a mut Project,
    id: &BitmapId,
) -> Result<&'a mut Bitmap, CommandError> {
    project
        .bitmaps
        .iter_mut()
        .find(|b| b.id == *id)
        .ok_or_else(|| CommandError::UnknownBitmap { id: id.clone() })
}

/// Sets the flags given on each of `ids`.
pub(super) fn set_flags(
    project: &mut Project,
    ids: &[BitmapId],
    visible: Option<bool>,
    locked: Option<bool>,
) -> Result<(), CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    if visible.is_none() && locked.is_none() {
        return Err(CommandError::NothingToDo);
    }
    for id in ids {
        let bitmap = find_mut(project, id)?;
        if let Some(visible) = visible {
            bitmap.visible = visible;
        }
        if let Some(locked) = locked {
            bitmap.locked = locked;
        }
    }
    Ok(())
}

/// Removes the bitmaps `ids`. A Photo V-Carve that carved one stays and
/// names a bitmap the project no longer has, which validation reports
/// (`photo_unknown_bitmap`; capability 56: "a Photo V-Carve that loses its
/// picture says so").
pub(super) fn delete(project: &mut Project, ids: &[BitmapId]) -> Result<(), CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    for id in ids {
        find_mut(project, id)?;
    }
    project.bitmaps.retain(|b| !ids.contains(&b.id));
    Ok(())
}

/// `“{name}”` for one bitmap, `{count} pictures` for several, as a label's
/// subject. An id the project does not have shows as the id.
pub(super) fn subject(strings: &Strings, project: &Project, ids: &[BitmapId]) -> String {
    match ids {
        [id] => {
            let name = project
                .bitmaps
                .iter()
                .find(|b| b.id == *id)
                .map_or_else(|| id.as_str().to_owned(), |b| b.name.clone());
            strings.fmt(StrKey::CommandSubjectOne, &[("name", &name)])
        }
        _ => strings.fmt(
            StrKey::CommandSubjectPictures,
            &[("count", &ids.len().to_string())],
        ),
    }
}
