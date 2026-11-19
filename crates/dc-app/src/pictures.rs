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
//! A picture "moves like a shape, and does not rotate, scale or mirror"
//! (capability 56): typing X or Y, dragging it or nudging it is one
//! [`Command::MoveBitmaps`] ([`edit`], [`move_by`]); W and H are shown
//! and not typed ([`read_only`]). A locked picture refuses, and the
//! refusal names it. The Photo V-Carve that carves a picture reads its
//! footprint, so it regenerates when the picture moves.
//!
//! The selection, the click that makes it and the rows' menu are
//! [`crate::App::click_picture`], [`crate::viewport::pick::picture_under`]
//! and [`crate::tree::bitmap_menu`].

use dc_geometry::{Point, Rect};
use dc_project::{Bitmap, BitmapId, Operation, Project};

use crate::command::{Command, check_bitmap_move};
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
/// locked, as a locked shape's are (capability 58); and W and H always,
/// since a picture is not scaled (capability 56).
#[must_use]
pub const fn read_only(properties: &PictureProperties, field: Field) -> bool {
    properties.placement.any_locked || matches!(field, Field::W | Field::H)
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

/// The command `edit`, typed into Properties, makes for the pictures
/// `ids`: X or Y moves their joint footprint's left or bottom edge there
/// ([`move_by`]).
///
/// # Errors
///
/// As [`move_by`], with [`CommandError::NothingToDo`] when the value is
/// the current one; [`CommandError::FieldRefused`] for W or H, which
/// [`read_only`] says are not typed.
pub fn edit(project: &Project, ids: &[BitmapId], edit: Edit) -> Result<Command, CommandError> {
    let Some(bounds) = bounds(project, ids) else {
        // Nothing selected, or an id the project lacks: `move_by` names it.
        return move_by(project, ids, 0.0, 0.0);
    };
    match edit {
        Edit::X { mm } => move_by(project, ids, mm - bounds.min.x, 0.0),
        Edit::Y { mm } => move_by(project, ids, 0.0, mm - bounds.min.y),
        Edit::W { .. } | Edit::H { .. } => Err(CommandError::FieldRefused {
            field: match edit {
                Edit::W { .. } => "width".to_owned(),
                _ => "height".to_owned(),
            },
            reason: "a picture is not scaled".to_owned(),
        }),
    }
}
