// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Properties panel's placement fields (`docs/specs/phase-4/design.md`,
//! "The Design Tree and placement"; functional spec, capability 58;
//! mock-up `01-workspace.svg`, Properties). With one or more instances
//! selected the panel shows, in project units, the selection's joint
//! bounding box (X and Y its bottom-left corner, W and H its size), the
//! rotation in degrees, and two Mirror buttons. A typed value becomes one
//! `TransformInstances` about the box's centre: a translation for X and Y,
//! a scale for W and H (uniform unless the padlock between them is open),
//! a rotation for the angle, a scale of −1 on one axis for a mirror. The
//! definitions are never written, so a linked copy's sibling stays.
//!
//! For several shapes the box is always one box, so X, Y, W and H are
//! never mixed; the rotation, Visible and Locked are each instance's own
//! and show *Mixed* when they differ ([`crate::mixed`], PureCutCNC's
//! `mixedValue.ts`). A typed angle over a mixed rotation turns the
//! selection by that angle.
//!
//! An instance's rotation is read from its transform's linear part as the
//! angle its X axis makes, counter-clockwise from +X in Y-up project
//! space: `atan2(b, a)`. A mirrored instance reads as the rotation that
//! follows its mirror in Y (`scale(−1, 1)` reads 180°).

use dc_geometry::{Rect, Transform, approx_eq};
use dc_project::{InstanceId, Project, Units};

use crate::command::{Command, centre, find_instance, selection_bounds};
use crate::error::CommandError;
use crate::library::trimmed;
use crate::mixed::{common_bool, common_number};
use crate::units;

/// Decimals a placement field shows, trailing zeros dropped: a thousandth
/// of an inch in mm, a tenth of a thou in inches (as the Tools tab's
/// fields).
const FIELD_DECIMALS: usize = 4;
/// Decimals the rotation shows.
const ANGLE_DECIMALS: usize = 2;
/// Two rotations within this many degrees are the same rotation, for
/// the mixed display.
const ANGLE_TOLERANCE_DEG: f64 = 1e-6;
/// A typed value within this of the current one, mm, changes nothing.
const NO_CHANGE_MM: f64 = 1e-9;
/// A box narrower than this, mm, has no width to scale (a straight line).
const MIN_EXTENT_MM: f64 = 1e-6;

/// One placement field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// The box's left edge.
    X,
    /// The box's bottom edge (Y-up).
    Y,
    /// The box's width.
    W,
    /// The box's height.
    H,
    /// The rotation, degrees counter-clockwise.
    Rotation,
}

/// A change typed into the panel, in millimetres and degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edit {
    /// Move the box's left edge to `mm`.
    X {
        /// The new X, mm.
        mm: f64,
    },
    /// Move the box's bottom edge to `mm`.
    Y {
        /// The new Y, mm.
        mm: f64,
    },
    /// Scale to width `mm` about the centre; the height follows when
    /// `uniform`.
    W {
        /// The new width, mm.
        mm: f64,
        /// Keep proportions (the padlock closed).
        uniform: bool,
    },
    /// Scale to height `mm` about the centre; the width follows when
    /// `uniform`.
    H {
        /// The new height, mm.
        mm: f64,
        /// Keep proportions (the padlock closed).
        uniform: bool,
    },
    /// Rotate about the centre by `angle_deg`, counter-clockwise.
    RotateBy {
        /// The turn, degrees.
        angle_deg: f64,
    },
    /// Mirror ↔: flip left to right about the centre.
    MirrorHorizontal,
    /// Mirror ↕: flip top to bottom about the centre.
    MirrorVertical,
}

/// What the panel shows of a selection of instances.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The joint bounding box of the selection's outlines, project space,
    /// mm.
    pub bounds: Rect,
    /// How many instances are selected.
    pub count: usize,
    /// The instances' common rotation, degrees in (−180, 180]; `None` when
    /// they differ.
    pub rotation_deg: Option<f64>,
    /// Every one visible, every one hidden, or mixed (`None`).
    pub visible: Option<bool>,
    /// Every one locked, every one unlocked, or mixed (`None`).
    pub locked: Option<bool>,
    /// Some instance is locked: the fields refuse, the handles are grey.
    pub any_locked: bool,
    /// The operations targeting any of them, by name, in project order
    /// (*Targeted by*).
    pub targeted_by: Vec<String>,
}

/// The rotation of `transform`'s linear part, degrees in (−180, 180].
#[must_use]
pub fn rotation_deg(transform: Transform) -> f64 {
    let [a, b, ..] = transform.coeffs();
    let deg = b.atan2(a).to_degrees();
    if approx_eq(deg, -180.0, ANGLE_TOLERANCE_DEG) || approx_eq(deg, 0.0, ANGLE_TOLERANCE_DEG) {
        deg.abs()
    } else {
        deg
    }
}

/// The placement of `ids` in `project`; `None` when nothing is selected
/// or the selection has no outline to bound.
#[must_use]
pub fn placement(project: &Project, ids: &[InstanceId]) -> Option<Placement> {
    let bounds = selection_bounds(project, ids).ok()?;
    let instances: Vec<_> = ids
        .iter()
        .filter_map(|id| find_instance(project, id).ok())
        .collect();
    Some(Placement {
        bounds,
        count: instances.len(),
        rotation_deg: common_number(
            &instances,
            |i| Some(rotation_deg(i.transform)),
            ANGLE_TOLERANCE_DEG,
        ),
        visible: common_bool(&instances, |i| Some(i.visible)),
        locked: common_bool(&instances, |i| Some(i.locked)),
        any_locked: instances.iter().any(|i| i.locked),
        targeted_by: project
            .operations
            .iter()
            .filter(|op| op.targets().iter().any(|t| ids.contains(t)))
            .map(|op| op.name().to_owned())
            .collect(),
    })
}

/// The text `field` shows, without its unit: lengths in `units`, the
/// rotation in degrees, empty when the rotation is mixed.
#[must_use]
pub fn field_text(placement: &Placement, field: Field, units: Units) -> String {
    let length = |mm: f64| trimmed(units::to_display(mm, units), FIELD_DECIMALS);
    let b = placement.bounds;
    match field {
        Field::X => length(b.min.x),
        Field::Y => length(b.min.y),
        Field::W => length(b.width_mm()),
        Field::H => length(b.height_mm()),
        Field::Rotation => placement
            .rotation_deg
            .map_or_else(String::new, |deg| trimmed(deg, ANGLE_DECIMALS)),
    }
}

/// The unit `field` shows inside its outline.
#[must_use]
pub const fn field_unit(field: Field, units: Units) -> &'static str {
    match field {
        Field::Rotation => "°",
        Field::X | Field::Y | Field::W | Field::H => units::suffix(units),
    }
}

/// A refusal of `text`.
fn refused(text: &str, reason: impl Into<String>) -> CommandError {
    CommandError::NotAQuantity {
        text: text.to_owned(),
        reason: reason.into(),
    }
}

/// `text` without a trailing `suffix`, ignoring case and spaces before it.
fn without<'a>(text: &'a str, suffix: &str) -> Option<&'a str> {
    let trimmed = text.trim();
    let cut = trimmed.len().checked_sub(suffix.len())?;
    let (head, tail) = (trimmed.get(..cut)?, trimmed.get(cut..)?);
    tail.eq_ignore_ascii_case(suffix).then(|| head.trim_end())
}

/// A plain decimal, as typed: a dot for the decimal point.
fn decimal(text: &str) -> Result<f64, CommandError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(refused(text, "nothing was typed"));
    }
    if trimmed.contains(',') {
        return Err(refused(text, "use a dot for the decimal point"));
    }
    match trimmed.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(value),
        _ => Err(refused(text, format!("`{trimmed}` is not a number"))),
    }
}

/// `deg` folded into (−180, 180].
fn fold_deg(deg: f64) -> f64 {
    let folded = (deg + 180.0).rem_euclid(360.0) - 180.0;
    if approx_eq(folded, -180.0, ANGLE_TOLERANCE_DEG) {
        180.0
    } else {
        folded
    }
}

/// What typing `text` into `field` asks for: X and Y take a length in
/// the file's unit strings (a bare number in `units`); W and H a length
/// or a percentage of the current size (`50 %`); the rotation degrees,
/// with or without `°`. `uniform` is the W/H padlock.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is not a value the field
/// takes.
pub fn parse(
    placement: &Placement,
    field: Field,
    text: &str,
    units: Units,
    uniform: bool,
) -> Result<Edit, CommandError> {
    let size = |current_mm: f64| -> Result<f64, CommandError> {
        match without(text, "%") {
            Some(percent) => Ok(current_mm * decimal(percent)? / 100.0),
            None => units::parse_length_mm(text, units),
        }
    };
    let b = placement.bounds;
    Ok(match field {
        Field::X => Edit::X {
            mm: units::parse_length_mm(text, units)?,
        },
        Field::Y => Edit::Y {
            mm: units::parse_length_mm(text, units)?,
        },
        Field::W => Edit::W {
            mm: size(b.width_mm())?,
            uniform,
        },
        Field::H => Edit::H {
            mm: size(b.height_mm())?,
            uniform,
        },
        Field::Rotation => {
            let bare = without(text, "°")
                .or_else(|| without(text, "deg"))
                .unwrap_or(text);
            let typed = decimal(bare)?;
            Edit::RotateBy {
                angle_deg: placement
                    .rotation_deg
                    .map_or(typed, |current| fold_deg(typed - current)),
            }
        }
    })
}

/// The scale factor that takes `current_mm` to `typed_mm`.
fn factor(field: &'static str, typed_mm: f64, current_mm: f64) -> Result<f64, CommandError> {
    if !typed_mm.is_finite() || typed_mm <= 0.0 {
        return Err(CommandError::InvalidValue {
            field,
            value: typed_mm,
        });
    }
    if current_mm < MIN_EXTENT_MM {
        return Err(CommandError::NothingToDo);
    }
    Ok(typed_mm / current_mm)
}

/// The command `edit` makes for `ids`: one `TransformInstances`, about
/// the selection's box centre.
///
/// # Errors
///
/// [`CommandError::NothingToDo`] when the value is the current one (no
/// undo step); [`CommandError::InvalidValue`] for a size that is not
/// positive; the selection's own errors (nothing selected, no outline).
/// A locked instance is refused when the command is applied.
pub fn edit(project: &Project, ids: &[InstanceId], edit: Edit) -> Result<Command, CommandError> {
    let b = selection_bounds(project, ids)?;
    let moved = |d: f64| -> Result<f64, CommandError> {
        if approx_eq(d, 0.0, NO_CHANGE_MM) {
            Err(CommandError::NothingToDo)
        } else {
            Ok(d)
        }
    };
    let scaled = |s: f64| -> Result<f64, CommandError> {
        if approx_eq(s, 1.0, NO_CHANGE_MM) {
            Err(CommandError::NothingToDo)
        } else {
            Ok(s)
        }
    };
    let delta = match edit {
        Edit::X { mm } => Transform::translate(moved(mm - b.min.x)?, 0.0),
        Edit::Y { mm } => Transform::translate(0.0, moved(mm - b.min.y)?),
        Edit::W { mm, uniform } => {
            let s = scaled(factor("width", mm, b.width_mm())?)?;
            Transform::scale(s, if uniform { s } else { 1.0 })
        }
        Edit::H { mm, uniform } => {
            let s = scaled(factor("height", mm, b.height_mm())?)?;
            Transform::scale(if uniform { s } else { 1.0 }, s)
        }
        Edit::RotateBy { angle_deg } => {
            if !angle_deg.is_finite() {
                return Err(CommandError::InvalidValue {
                    field: "rotation",
                    value: angle_deg,
                });
            }
            moved(fold_deg(angle_deg))?;
            Transform::rotate(angle_deg.to_radians())
        }
        Edit::MirrorHorizontal => Transform::scale(-1.0, 1.0),
        Edit::MirrorVertical => Transform::scale(1.0, -1.0),
    };
    Ok(Command::TransformInstances {
        ids: ids.to_vec(),
        delta,
    })
}

/// The centre of the selection's box, mm: the pivot of every placement
/// change.
#[must_use]
pub fn pivot(placement: &Placement) -> dc_geometry::Point {
    centre(placement.bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_read_counter_clockwise_and_fold() {
        let quarter = Transform::rotate(std::f64::consts::FRAC_PI_2);
        assert!(approx_eq(rotation_deg(quarter), 90.0, 1e-9));
        assert!(approx_eq(rotation_deg(Transform::IDENTITY), 0.0, 1e-12));
        assert!(approx_eq(
            rotation_deg(Transform::scale(-1.0, 1.0)),
            180.0,
            1e-12
        ));
        assert!(approx_eq(fold_deg(270.0), -90.0, 1e-12));
        assert!(approx_eq(fold_deg(-180.0), 180.0, 1e-12));
        assert!(approx_eq(fold_deg(360.0), 0.0, 1e-12));
    }

    #[test]
    fn suffixes_come_off_whatever_their_case() {
        assert_eq!(without("50 %", "%"), Some("50"));
        assert_eq!(without("45DEG", "deg"), Some("45"));
        assert_eq!(without("45", "deg"), None);
        assert_eq!(without("", "%"), None);
    }
}
