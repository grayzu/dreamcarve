// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A picture as a thing to select and place (functional spec, capabilities
//! 56 to 58; slice 15, #544). A bitmap is a row of the Design Tree
//! ([`crate::tree::BitmapRow`]) and a textured quad on the Viewport
//! ([`crate::scene::SceneBitmap`]); here are the Properties of the
//! pictures selected ([`crate::App::selected_pictures`]) and the commands
//! that place them.
//!
//! Properties shows a picture as it shows a shape (capability 58): X and
//! Y, the bottom-left corner of its footprint, then W and H, with the same
//! fields, text and parsing ([`crate::properties`]), so [`properties`]
//! gives a [`Placement`]. Beside them are what only a picture has: its
//! size in pixels, read-only ([`pixels_text`]), and, as *Targeted by*, the
//! Photo V-Carves that carve it. Several pictures show their joint box.
//!
//! A picture moves like a shape (capability 56): typing X or Y, dragging
//! it or nudging it is one [`Command::MoveBitmaps`] ([`edit`],
//! [`move_by`]). It does not rotate or mirror, and it is never stretched:
//! one picture's W or H can be retyped, and the other side follows its
//! pixels, as at import, about the footprint's centre
//! ([`Command::ResizeBitmap`], [`resize`]), so the proportion link is
//! always closed. Several pictures have no one size to type
//! ([`read_only`]). A locked picture refuses both, and the refusal names
//! it. The Photo V-Carve that carves a picture reads its footprint, so it
//! regenerates when the picture moves or changes size.
//!
//! The selection, the click that makes it and the rows' menu are
//! [`crate::App::click_picture`], [`crate::viewport::pick::Hit::Bitmap`]
//! and [`crate::tree::bitmap_menu`].

use dc_geometry::{Point, Rect};
use dc_job::picture::PictureSize;
use dc_project::{Bitmap, BitmapId, Operation, Project};

use crate::command::{Command, check_bitmap_move, check_bitmap_resize};
use crate::error::CommandError;
use crate::mixed::common_bool;
use crate::properties::{Edit, Field, Placement};
use crate::strings::{StrKey, Strings};

/// The label of the read-only row that shows a picture's size in pixels.
pub const PIXELS_LABEL: StrKey = StrKey::PlacementPicturePixels;

/// What the Properties panel shows of a selection of pictures.
#[derive(Debug, Clone, PartialEq)]
pub struct PictureProperties {
    /// The selection's joint footprint and flags, as a selection of
    /// shapes' ([`crate::properties::field_text`] and
    /// [`crate::properties::parse`] read it): its rotation is always 0,
    /// and `targeted_by` names the Photo V-Carves that carve any of the
    /// pictures, in project order.
    pub placement: Placement,
    /// The one selected picture's name; `None` for several.
    pub name: Option<String>,
    /// The one selected picture's size, pixels, width then height; `None`
    /// for several.
    pub pixels: Option<(u32, u32)>,
}

/// `bitmap`'s footprint in project space, mm, Y-up.
fn footprint(bitmap: &Bitmap) -> Rect {
    Rect {
        min: Point::new(bitmap.x_mm, bitmap.y_mm),
        max: Point::new(
            bitmap.x_mm + bitmap.width_mm,
            bitmap.y_mm + bitmap.height_mm,
        ),
    }
}

/// The bitmaps of `project` among `ids`, in selection order; an id the
/// project does not have is left out.
fn chosen<'p>(project: &'p Project, ids: &[BitmapId]) -> Vec<&'p Bitmap> {
    ids.iter()
        .filter_map(|id| project.bitmaps.iter().find(|b| b.id == *id))
        .collect()
}

/// The joint footprint of the pictures `ids`, mm: the box the Viewport
/// draws round the selection and what *Zoom to* frames. A hidden picture
/// counts: it is still placed. `None` when the project has none of them.
#[must_use]
pub fn bounds(project: &Project, ids: &[BitmapId]) -> Option<Rect> {
    chosen(project, ids)
        .into_iter()
        .map(footprint)
        .reduce(|joint, next| joint.including(next.min).including(next.max))
}

/// The Properties of the pictures `ids`; `None` when nothing is selected
/// or the project has none of them.
#[must_use]
pub fn properties(project: &Project, ids: &[BitmapId]) -> Option<PictureProperties> {
    let pictures = chosen(project, ids);
    let bounds = bounds(project, ids)?;
    let one = match pictures.as_slice() {
        [one] => Some(*one),
        _ => None,
    };
    Some(PictureProperties {
        placement: Placement {
            bounds,
            count: pictures.len(),
            // A picture never turns (capability 56).
            rotation_deg: Some(0.0),
            visible: common_bool(&pictures, |b| Some(b.visible)),
            locked: common_bool(&pictures, |b| Some(b.locked)),
            any_locked: pictures.iter().any(|b| b.locked),
            targeted_by: project
                .operations
                .iter()
                .filter(|op| {
                    matches!(op, Operation::PhotoVCarve(photo)
                        if pictures.iter().any(|b| b.id == photo.image))
                })
                .map(|op| op.name().to_owned())
                .collect(),
        },
        name: one.map(|b| b.name.clone()),
        pixels: one.map(|b| (b.width_px, b.height_px)),
    })
}

/// The panel's title: *Properties · gradient* for one picture,
/// *Properties · 2 pictures* for several.
#[must_use]
pub fn title(strings: &Strings, properties: &PictureProperties) -> String {
    match &properties.name {
        Some(name) => strings.fmt(StrKey::PlacementTitle, &[("name", name)]),
        None => strings.fmt(
            StrKey::PlacementPictureTitleMany,
            &[("count", &properties.placement.count.to_string())],
        ),
    }
}

/// What the [`PIXELS_LABEL`] row shows: the one picture's size in pixels,
/// `64 × 32 px`. Read-only: the pixels are the picture. `None` for
/// several, whose row is not shown.
#[must_use]
pub fn pixels_text(strings: &Strings, properties: &PictureProperties) -> Option<String> {
    let (width_px, height_px) = properties.pixels?;
    Some(strings.fmt(
        StrKey::PlacementPicturePixelsValue,
        &[
            ("width", &width_px.to_string()),
            ("height", &height_px.to_string()),
        ],
    ))
}

/// Whether `field` is read-only: every field while a selected picture is
/// locked, as a locked shape's are (capability 58); and W and H while
/// several pictures are selected, since a size is one picture's
/// (capability 56). The proportion link beside W and H is always closed
/// for a picture, and is not a control.
#[must_use]
pub const fn read_only(properties: &PictureProperties, field: Field) -> bool {
    properties.placement.any_locked
        || (matches!(field, Field::W | Field::H) && properties.placement.count != 1)
}

/// The locked note's *Unlock*: one step unlocking the locked ones of
/// `ids`; `None` when none is locked.
#[must_use]
pub fn unlock(project: &Project, ids: &[BitmapId]) -> Option<Command> {
    let locked: Vec<BitmapId> = chosen(project, ids)
        .into_iter()
        .filter(|b| b.locked)
        .map(|b| b.id.clone())
        .collect();
    (!locked.is_empty()).then_some(Command::SetBitmapFlags {
        ids: locked,
        visible: None,
        locked: Some(false),
    })
}

/// The pictures `ids` moved by (`dx_mm`, `dy_mm`) as one command: where a
/// drag on the Viewport ended, or an arrow key's nudge.
///
/// # Errors
///
/// What [`Command::MoveBitmaps`] would refuse, found before it runs so
/// the status bar can say why: [`CommandError::NothingSelected`],
/// [`CommandError::UnknownBitmap`], [`CommandError::PictureLocked`]
/// naming the locked pictures, [`CommandError::NothingToDo`] for no
/// distance (no undo step), [`CommandError::InvalidValue`] for a distance
/// that is not finite, and [`CommandError::Job`] for a footprint moved
/// past the coordinate limit.
pub fn move_by(
    project: &Project,
    ids: &[BitmapId],
    dx_mm: f64,
    dy_mm: f64,
) -> Result<Command, CommandError> {
    check_bitmap_move(project, ids, dx_mm, dy_mm)?;
    Ok(Command::MoveBitmaps {
        ids: ids.to_vec(),
        dx_mm,
        dy_mm,
    })
}

/// Picture `id` at `size`, a width or a height, as one command: the
/// other side follows the pixels and the footprint keeps its centre.
///
/// # Errors
///
/// What [`Command::ResizeBitmap`] would refuse, found before it runs:
/// [`CommandError::UnknownBitmap`], [`CommandError::PictureLocked`],
/// [`CommandError::Job`] for a size that is not a positive length or a
/// footprint past the coordinate limit, and [`CommandError::NothingToDo`]
/// for the size it has (no undo step).
pub fn resize(
    project: &Project,
    id: &BitmapId,
    size: PictureSize,
) -> Result<Command, CommandError> {
    check_bitmap_resize(project, id, size)?;
    Ok(Command::ResizeBitmap {
        id: id.clone(),
        size,
    })
}

/// The command `edit`, typed into Properties, makes for the pictures
/// `ids`: X or Y moves their joint footprint's left or bottom edge there
/// ([`move_by`]); W or H resizes the one picture selected ([`resize`]).
/// An edit's `uniform` is not read: a picture keeps its proportions
/// whatever the link shows.
///
/// # Errors
///
/// As [`move_by`] and [`resize`], with [`CommandError::NothingToDo`]
/// when the value is the current one; [`CommandError::FieldRefused`] for
/// W or H with several pictures selected, which [`read_only`] says are
/// not typed.
pub fn edit(project: &Project, ids: &[BitmapId], edit: Edit) -> Result<Command, CommandError> {
    let Some(bounds) = bounds(project, ids) else {
        // Nothing selected, or an id the project lacks: `move_by` names it.
        return move_by(project, ids, 0.0, 0.0);
    };
    let (field, size) = match edit {
        Edit::X { mm } => return move_by(project, ids, mm - bounds.min.x, 0.0),
        Edit::Y { mm } => return move_by(project, ids, 0.0, mm - bounds.min.y),
        Edit::W { mm, .. } => ("width", PictureSize::Width { width_mm: mm }),
        Edit::H { mm, .. } => ("height", PictureSize::Height { height_mm: mm }),
    };
    match ids {
        [id] => resize(project, id, size),
        _ => Err(CommandError::FieldRefused {
            field: field.to_owned(),
            reason: "several pictures are selected, and a size is one picture's".to_owned(),
        }),
    }
}
