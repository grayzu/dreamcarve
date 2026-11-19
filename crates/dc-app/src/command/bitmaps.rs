// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The commands on a project's bitmaps (functional spec, capabilities 56
//! and 57): a picture imported as `dreamcarve image add` adds it, and a
//! Pictures row's eye, padlock, name and *Delete*.

use dc_job::picture::{PictureSize, picture_into};
use dc_project::{Bitmap, BitmapId, Grayscale, Project};

use crate::error::CommandError;
use crate::import::{PICTURE_CORNER, stem};
use crate::strings::{StrKey, Strings};

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
