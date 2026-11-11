// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Arrange actions (functional spec, capability 58a; slice 25, #617):
//! rotate the selection 90° either way or by a typed angle, flip it
//! horizontally or vertically, or centre it on the stock. Rotation and
//! mirroring are actions on the selection, not properties of a shape
//! (58), so the toolbar's Arrange group, the Arrange menu and the
//! shortcuts R, Shift R, Ctrl Shift H and Ctrl Shift V all ask for an
//! [`Action`] and this module turns it into one command.
//!
//! A lock stops transforms only (57): with some selected shapes locked,
//! the unlocked ones turn about their own joint bounding-box centre and
//! [`Arranged::locked`] names the rest for the status bar. With every one
//! locked, nothing selected, or only pictures (which cannot turn, 56), the
//! group is [`Disabled`] with a reason. The angle follows the Y-up file:
//! positive anticlockwise.

use dc_project::{InstanceId, Project};

use crate::command::{Command, find_instance};
use crate::error::CommandError;
use crate::strings::{StrKey, Strings};

/// A quarter turn, degrees.
const QUARTER_DEG: f64 = 90.0;

/// The axis a flip mirrors across, named as the toolbar names the flip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flip {
    /// Left to right: a scale of −1 in X about the box's centre.
    Horizontal,
    /// Top to bottom: a scale of −1 in Y about the box's centre.
    Vertical,
}

/// What the Arrange group, menu or a shortcut asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// The toolbar's Rotate, R: 90° clockwise.
    RotateClockwise,
    /// Shift R: 90° anticlockwise.
    RotateAnticlockwise,
    /// The Rotate popover's angle, degrees, positive anticlockwise.
    Rotate {
        /// The turn, degrees.
        angle_deg: f64,
    },
    /// Flip horizontal, Ctrl Shift H.
    FlipHorizontal,
    /// Flip vertical, Ctrl Shift V.
    FlipVertical,
    /// Arrange ▸ Centre on Stock (capability 61).
    CentreOnStock,
}

/// Why the Arrange group is disabled, shown in its tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disabled {
    /// No shape is selected.
    NothingSelected,
    /// Every selected shape is locked.
    AllLocked,
    /// Only pictures are selected, and a picture cannot turn.
    OnlyPictures,
}

impl Disabled {
    /// The reason's string key.
    #[must_use]
    pub const fn reason(self) -> StrKey {
        match self {
            Self::NothingSelected => StrKey::ArrangeDisabledNothing,
            Self::AllLocked => StrKey::ArrangeDisabledLocked,
            Self::OnlyPictures => StrKey::ArrangeDisabledPictures,
        }
    }
}

/// An action turned into its command.
#[derive(Debug, Clone, PartialEq)]
pub struct Arranged {
    /// The one command: it names only the unlocked shapes.
    pub command: Command,
    /// The selected shapes that are locked and stay, by name, in
    /// selection order.
    pub locked: Vec<String>,
}

/// Whether the group is disabled for a selection of `instances` and
/// `pictures` pictures, and why.
#[must_use]
pub fn disabled(project: &Project, instances: &[InstanceId], pictures: usize) -> Option<Disabled> {
    if instances.is_empty() {
        return Some(if pictures > 0 {
            Disabled::OnlyPictures
        } else {
            Disabled::NothingSelected
        });
    }
    let all_locked = instances
        .iter()
        .filter_map(|id| find_instance(project, id).ok())
        .all(|i| i.locked);
    all_locked.then_some(Disabled::AllLocked)
}

/// The command `action` makes of the selection: the unlocked instances
/// only, with the locked ones named.
///
/// # Errors
///
/// [`Disabled`] when nothing in the selection can turn.
pub fn arrange(
    project: &Project,
    instances: &[InstanceId],
    pictures: usize,
    action: Action,
) -> Result<Arranged, Disabled> {
    if let Some(why) = disabled(project, instances, pictures) {
        return Err(why);
    }
    let mut ids = Vec::new();
    let mut locked = Vec::new();
    for id in instances {
        match find_instance(project, id) {
            Ok(i) if i.locked => locked.push(i.name.clone()),
            _ => ids.push(id.clone()),
        }
    }
    let rotate = |angle_deg| Command::RotateInstances {
        ids: ids.clone(),
        angle_deg,
    };
    let flip = |flip| Command::FlipInstances {
        ids: ids.clone(),
        flip,
    };
    let command = match action {
        Action::RotateClockwise => rotate(-QUARTER_DEG),
        Action::RotateAnticlockwise => rotate(QUARTER_DEG),
        Action::Rotate { angle_deg } => rotate(angle_deg),
        Action::FlipHorizontal => flip(Flip::Horizontal),
        Action::FlipVertical => flip(Flip::Vertical),
        Action::CentreOnStock => Command::CentreOnStock(ids.clone()),
    };
    Ok(Arranged { command, locked })
}

/// The status bar's line naming the locked shapes that stayed; `None`
/// when none did.
#[must_use]
pub fn locked_message(strings: &Strings, locked: &[String]) -> Option<String> {
    match locked {
        [] => None,
        [one] => Some(strings.fmt(StrKey::ArrangeLockedOne, &[("name", one)])),
        many => Some(strings.fmt(
            StrKey::ArrangeLockedMany,
            &[
                ("count", &many.len().to_string()),
                ("names", &many.join(", ")),
            ],
        )),
    }
}

/// The Rotate popover's typed angle, degrees: a plain decimal with a dot,
/// with or without `°`.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is not a finite number.
pub fn parse_angle(text: &str) -> Result<f64, CommandError> {
    let trimmed = text.trim();
    let bare = trimmed.strip_suffix('°').unwrap_or(trimmed).trim_end();
    let refused = |reason: String| CommandError::NotAQuantity {
        text: text.to_owned(),
        reason,
    };
    if bare.is_empty() {
        return Err(refused("nothing was typed".to_owned()));
    }
    if bare.contains(',') {
        return Err(refused("use a dot for the decimal point".to_owned()));
    }
    match bare.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(value),
        _ => Err(refused(format!("`{bare}` is not a number of degrees"))),
    }
}
