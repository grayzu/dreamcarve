// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The commands on a project's bitmaps (functional spec, capabilities 56
//! and 57): a picture imported as `dreamcarve image add` adds it, and a
//! Pictures row's eye, padlock, name and *Delete*; and a picture moved as
//! a shape is, or its size retyped with its proportions kept (slice 15,
//! #544; capability 58).

use dc_geometry::{MAX_COORDINATE_MM, approx_eq};
use dc_job::JobError;
use dc_job::picture::{PictureSize, picture_into};
use dc_project::{Bitmap, BitmapId, Grayscale, MIN_BITMAP_OPACITY, Project, round_length_mm};

use crate::error::CommandError;
use crate::import::{PICTURE_CORNER, stem};
use crate::strings::{StrKey, Strings};

/// A move shorter than this on both axes, mm, moves nothing.
const NO_MOVE_MM: f64 = 1e-9;

/// Two opacities nearer than this are one.
const SAME_OPACITY: f64 = 1e-9;

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

/// The footprint `bitmap` has at `size` about its own centre: its
/// bottom-left corner's `(x_mm, y_mm)`, then `(width_mm, height_mm)`, the
/// side not given following the pixels as at import
/// ([`PictureSize::footprint_mm`]), each rounded as a stored length is.
fn resized(bitmap: &Bitmap, size: PictureSize) -> Result<[f64; 4], CommandError> {
    let (width_mm, height_mm) = size.footprint_mm(bitmap.width_px, bitmap.height_px)?;
    let centre_x_mm = bitmap.x_mm + bitmap.width_mm / 2.0;
    let centre_y_mm = bitmap.y_mm + bitmap.height_mm / 2.0;
    let x_mm = round_length_mm(centre_x_mm - width_mm / 2.0);
    let y_mm = round_length_mm(centre_y_mm - height_mm / 2.0);
    in_range(x_mm, y_mm, width_mm, height_mm)?;
    Ok([x_mm, y_mm, width_mm, height_mm])
}

/// Whether `ResizeBitmap` can set bitmap `id` to `size`, and the
/// footprint it then has ([`resized`]): the project has it, it is not
/// locked, the size is a positive length within the coordinate limit, and
/// it is not the size it has.
fn resize_to(
    project: &Project,
    id: &BitmapId,
    size: PictureSize,
) -> Result<[f64; 4], CommandError> {
    let bitmap = find(project, id)?;
    refuse_locked(project, std::slice::from_ref(id))?;
    let to = resized(bitmap, size)?;
    let [_, _, width_mm, height_mm] = to;
    if approx_eq(width_mm, bitmap.width_mm, NO_MOVE_MM)
        && approx_eq(height_mm, bitmap.height_mm, NO_MOVE_MM)
    {
        return Err(CommandError::NothingToDo);
    }
    Ok(to)
}

/// Whether `ResizeBitmap` can set bitmap `id` to `size` ([`resize_to`]).
pub(crate) fn check_resize(
    project: &Project,
    id: &BitmapId,
    size: PictureSize,
) -> Result<(), CommandError> {
    resize_to(project, id, size).map(|_| ())
}

/// `ResizeBitmap`: bitmap `id` at `size` about its centre, its pixels
/// untouched ([`resize_to`]).
pub(super) fn resize(
    project: &mut Project,
    id: &BitmapId,
    size: PictureSize,
) -> Result<(), CommandError> {
    let to = resize_to(project, id, size)?;
    set_footprint(find_mut(project, id)?, to);
    Ok(())
}

/// Gives `bitmap` the footprint `to`: its corner, then its size.
fn set_footprint(bitmap: &mut Bitmap, to: [f64; 4]) {
    [bitmap.x_mm, bitmap.y_mm, bitmap.width_mm, bitmap.height_mm] = to;
}

/// Whether `PlaceBitmap` can put bitmap `id` at (`x_mm`, `y_mm`),
/// `width_mm` wide, and the footprint it then has: the project has it,
/// it is not locked, the width is a positive length and the footprint
/// within the coordinate limit, and it is not where it is already. The
/// height follows the pixels, as at import.
fn place_at(
    project: &Project,
    id: &BitmapId,
    x_mm: f64,
    y_mm: f64,
    width_mm: f64,
) -> Result<[f64; 4], CommandError> {
    let bitmap = find(project, id)?;
    refuse_locked(project, std::slice::from_ref(id))?;
    super::finite("bitmaps[].x_mm", x_mm)?;
    super::finite("bitmaps[].y_mm", y_mm)?;
    let (width_mm, height_mm) =
        PictureSize::Width { width_mm }.footprint_mm(bitmap.width_px, bitmap.height_px)?;
    let (x_mm, y_mm) = (round_length_mm(x_mm), round_length_mm(y_mm));
    in_range(x_mm, y_mm, width_mm, height_mm)?;
    let to = [x_mm, y_mm, width_mm, height_mm];
    let from = [bitmap.x_mm, bitmap.y_mm, bitmap.width_mm, bitmap.height_mm];
    if to
        .iter()
        .zip(from)
        .all(|(to, from)| approx_eq(*to, from, NO_MOVE_MM))
    {
        return Err(CommandError::NothingToDo);
    }
    Ok(to)
}

/// Whether `PlaceBitmap` can put bitmap `id` there ([`place_at`]).
pub(crate) fn check_place(
    project: &Project,
    id: &BitmapId,
    x_mm: f64,
    y_mm: f64,
    width_mm: f64,
) -> Result<(), CommandError> {
    place_at(project, id, x_mm, y_mm, width_mm).map(|_| ())
}

/// `PlaceBitmap`: bitmap `id` at a corner and a width, its pixels
/// untouched ([`place_at`]).
pub(super) fn place(
    project: &mut Project,
    id: &BitmapId,
    x_mm: f64,
    y_mm: f64,
    width_mm: f64,
) -> Result<(), CommandError> {
    let to = place_at(project, id, x_mm, y_mm, width_mm)?;
    set_footprint(find_mut(project, id)?, to);
    Ok(())
}

/// Whether `SetBitmapOpacity` can set `ids` to `opacity_fraction`: some
/// are named, the project has each, the opacity is one the file takes
/// ([`MIN_BITMAP_OPACITY`] to 1), and it is not the one they all have. A
/// lock does not refuse it: a lock stops a picture moving, and its
/// opacity moves nothing.
pub(crate) fn check_opacity(
    project: &Project,
    ids: &[BitmapId],
    opacity_fraction: f64,
) -> Result<(), CommandError> {
    const FIELD: &str = "bitmaps[].opacity_fraction";
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    super::finite(FIELD, opacity_fraction)?;
    if !(MIN_BITMAP_OPACITY..=1.0).contains(&opacity_fraction) {
        return Err(CommandError::InvalidValue {
            field: FIELD,
            value: opacity_fraction,
        });
    }
    let mut changes = false;
    for id in ids {
        changes |= !approx_eq(
            find(project, id)?.opacity_fraction,
            opacity_fraction,
            SAME_OPACITY,
        );
    }
    if changes {
        Ok(())
    } else {
        Err(CommandError::NothingToDo)
    }
}

/// `SetBitmapOpacity`: each of `ids` at `opacity_fraction`
/// ([`check_opacity`]).
pub(super) fn set_opacity(
    project: &mut Project,
    ids: &[BitmapId],
    opacity_fraction: f64,
) -> Result<(), CommandError> {
    check_opacity(project, ids, opacity_fraction)?;
    for bitmap in project.bitmaps.iter_mut().filter(|b| ids.contains(&b.id)) {
        bitmap.opacity_fraction = opacity_fraction;
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
