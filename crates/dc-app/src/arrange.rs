// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Arrange actions (functional spec, capability 58a; slice 25, #617):
//! rotate the selection 90° either way or by a typed angle, flip it
//! horizontally or vertically, or centre it on the stock. Rotation and
//! mirroring are actions on the selection, not properties of a shape
//! (58), so the Inspector's Arrange group, the Arrange menu and the
//! shortcuts R, Shift R, Ctrl Shift H and Ctrl Shift V all ask for an
//! [`Action`] and this module turns it into one command.
//!
//! A lock stops transforms only (57): with some selected shapes locked,
//! the unlocked ones turn about their own joint bounding-box centre and
//! [`Arranged::locked`] names the rest for the status bar. With every one
//! locked, nothing selected, or only pictures (which cannot turn, 56), the
//! group is [`Disabled`] with a reason. The angle follows the Y-up file:
//! positive anticlockwise.

use dc_geometry::Transform;
use dc_project::{InstanceId, Project};

pub use crate::align::{Align, Axis, Spacing};
use crate::command::{Command, Placing, find_instance};
use crate::error::CommandError;
use crate::origin::Origin;
use crate::strings::{StrKey, Strings};

/// A quarter turn, degrees.
const QUARTER_DEG: f64 = 90.0;

/// The axis a flip mirrors across, named as the Arrange group names the flip.
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
    /// The Arrange group's Rotate, R: 90° clockwise.
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
    /// Align the shapes by a side of their boxes (phase 5, capability
    /// 98a): to the selection's box, or to the stock when `to_stock` or
    /// when one shape is selected.
    Align {
        /// The side.
        side: Align,
        /// Align to the stock whatever is selected.
        to_stock: bool,
    },
    /// Space three shapes or more evenly between the outermost two
    /// (capability 98a).
    Distribute {
        /// Along X or along Y.
        axis: Axis,
        /// Equal gaps, or equal distances between middles.
        spacing: Spacing,
    },
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
    /// Distribute was asked of fewer than three unlocked shapes.
    TooFewToDistribute,
}

impl Disabled {
    /// The reason's string key.
    #[must_use]
    pub const fn reason(self) -> StrKey {
        match self {
            Self::NothingSelected => StrKey::ArrangeDisabledNothing,
            Self::AllLocked => StrKey::ArrangeDisabledLocked,
            Self::OnlyPictures => StrKey::ArrangeDisabledPictures,
            Self::TooFewToDistribute => StrKey::ArrangeDisabledDistribute,
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
    arrange_about(project, instances, pictures, action, Origin::Centre)
}

/// [`arrange`] with a turn or a flip made about `origin`, which stays
/// where it is ([`crate::origin`]). About the centre they are the
/// commands they always were, `RotateInstances` and `FlipInstances`;
/// about another point each is one `TransformInstances` of the turn or
/// the mirror and the move that keeps the point, labelled *Rotate* or
/// *Mirror*. Align, Distribute and Centre on Stock read no origin.
///
/// # Errors
///
/// As [`arrange`].
pub fn arrange_about(
    project: &Project,
    instances: &[InstanceId],
    pictures: usize,
    action: Action,
    origin: Origin,
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
    // The box of what is turned: the unlocked shapes, as the command's.
    let pivoted = (origin != Origin::Centre)
        .then(|| crate::command::selection_bounds(project, &ids).ok())
        .flatten();
    let about = |change: Transform| {
        pivoted.map(|bounds| Command::TransformInstances {
            ids: ids.clone(),
            delta: origin.about(bounds, change),
        })
    };
    let rotate = |angle_deg: f64| {
        about(Transform::rotate(angle_deg.to_radians())).unwrap_or_else(|| {
            Command::RotateInstances {
                ids: ids.clone(),
                angle_deg,
            }
        })
    };
    let flip = |flip: Flip| {
        let mirror = match flip {
            Flip::Horizontal => Transform::scale(-1.0, 1.0),
            Flip::Vertical => Transform::scale(1.0, -1.0),
        };
        about(mirror).unwrap_or_else(|| Command::FlipInstances {
            ids: ids.clone(),
            flip,
        })
    };
    let command = match action {
        Action::RotateClockwise => rotate(-QUARTER_DEG),
        Action::RotateAnticlockwise => rotate(QUARTER_DEG),
        Action::Rotate { angle_deg } => rotate(angle_deg),
        Action::FlipHorizontal => flip(Flip::Horizontal),
        Action::FlipVertical => flip(Flip::Vertical),
        Action::CentreOnStock => Command::CentreOnStock(ids.clone()),
        Action::Align { side, to_stock } => Command::PlaceInstances {
            moves: crate::align::align(project, instances, &ids, side, to_stock),
            what: Placing::Align,
        },
        Action::Distribute { axis, spacing } => {
            if ids.len() < 3 {
                return Err(Disabled::TooFewToDistribute);
            }
            Command::PlaceInstances {
                moves: crate::align::distribute(project, &ids, axis, spacing),
                what: Placing::Distribute,
            }
        }
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
