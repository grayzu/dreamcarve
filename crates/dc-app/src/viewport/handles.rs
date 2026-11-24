// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The transform handles on the Viewport (`docs/specs/phase-4/design.md`,
//! "Data model → dc-app", `Handles` and `drag_delta`; functional spec,
//! capability 58; `experience.md`, section 8, "Transform handles"): the
//! selection's joint box with a handle at each corner and a rotation
//! handle on a 20-px stem above the top's centre. A corner drag scales
//! about the box's centre, keeping proportions unless the W/H padlock is
//! open (or Alt is held); the rotation handle turns the selection about
//! the centre, in 15° steps with Shift; a drag from a shape moves it.
//!
//! [`drag_delta`] is the change as `TransformInstances` takes it, about
//! the selection's centre, which is the centre these handles sit round;
//! [`about_centre`] is the same change placed there, for the live preview
//! the shell draws while the button is down. Positions are project
//! millimetres, Y up; the one flip is the camera's.

use dc_geometry::{Point, Rect, Transform, approx_eq};
use dc_project::{InstanceId, Project, Units};

use crate::command::{about, centre, selection_bounds};
use crate::error::CommandError;
use crate::origin::Origin;
use crate::strings::{StrKey, Strings};
use crate::tool::Label;
use crate::viewport::camera::{Camera, Pos};

/// The rotation handle's stem above the box's top edge, logical pixels.
pub const STEM_PX: f64 = 20.0;
/// How far from a handle's centre a press still grabs it, logical pixels:
/// the 8-px square's half plus a little slack.
pub const HANDLE_HIT_PX: f64 = 6.0;
/// The shortest side, logical pixels, that carries a handle at its
/// middle: under it the handle would crowd the two at the corners.
pub const MIN_SIDE_PX: f64 = 28.0;
/// The rotation step with Shift held, degrees.
pub const SNAP_DEG: f64 = 15.0;
/// The smallest scale a corner drag gives: dragged through the centre,
/// the selection shrinks to this rather than collapsing or flipping.
pub const MIN_SCALE: f64 = 0.01;
/// A pointer this close to the centre, mm, gives no direction to scale
/// or turn by.
const DEGENERATE_MM: f64 = 1e-9;

/// A corner of the selection's box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    /// Least X, least Y.
    BottomLeft,
    /// Greatest X, least Y.
    BottomRight,
    /// Greatest X, greatest Y.
    TopRight,
    /// Least X, greatest Y.
    TopLeft,
}

impl Corner {
    /// The four, in [`Handles::corners`]' order.
    pub const ALL: [Self; 4] = [
        Self::BottomLeft,
        Self::BottomRight,
        Self::TopRight,
        Self::TopLeft,
    ];

    const fn index(self) -> usize {
        match self {
            Self::BottomLeft => 0,
            Self::BottomRight => 1,
            Self::TopRight => 2,
            Self::TopLeft => 3,
        }
    }
}

/// A side of the selection's box, whose middle carries a handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Least X.
    Left,
    /// Greatest X.
    Right,
    /// Least Y.
    Bottom,
    /// Greatest Y.
    Top,
}

impl Side {
    /// The four, in the order they are tried under the pointer.
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Bottom, Self::Top];
}

/// What a press on the Viewport took hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    /// The selection itself: a drag moves it.
    Move,
    /// A corner handle: a drag scales about the centre.
    Corner(Corner),
    /// The handle at the middle of a side: a drag stretches the
    /// selection along one axis, that side following the pointer and
    /// the opposite side staying where it is.
    Side(Side),
    /// The rotation handle: a drag turns about the centre, in
    /// [`SNAP_DEG`] steps when `snap`.
    Rotate {
        /// Shift held.
        snap: bool,
    },
}

/// The handles of a selection, in project millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handles {
    /// The selection's joint box.
    pub bounds: Rect,
    /// Its corners: bottom-left, bottom-right, top-right, top-left.
    pub corners: [Point; 4],
    /// The rotation handle, [`STEM_PX`] above the top edge's centre at the
    /// camera's scale.
    pub rotation: Point,
    /// Some instance in the selection is locked: the handles are grey and
    /// every drag is refused.
    pub locked: bool,
    /// The point a corner's scale and the rotation handle's turn are
    /// about: the box's centre, until [`Handles::about`] sets the
    /// selection's origin ([`crate::origin`]).
    pub pivot: Point,
}

impl Handles {
    /// The handle at `corner`.
    #[must_use]
    pub fn corner(&self, corner: Corner) -> Point {
        self.corners
            .get(corner.index())
            .copied()
            .unwrap_or(self.bounds.min)
    }

    /// The handle at the middle of `side`.
    #[must_use]
    pub fn side(&self, side: Side) -> Point {
        let (min, max) = (self.bounds.min, self.bounds.max);
        let middle = centre(self.bounds);
        match side {
            Side::Left => Point::new(min.x, middle.y),
            Side::Right => Point::new(max.x, middle.y),
            Side::Bottom => Point::new(middle.x, min.y),
            Side::Top => Point::new(middle.x, max.y),
        }
    }

    /// The sides that carry a handle at `camera`'s scale: those of a box
    /// that has a width and a height, and long enough on screen
    /// ([`MIN_SIDE_PX`]) for a handle to sit clear of the corners'.
    #[must_use]
    pub fn sides(&self, camera: &Camera) -> Vec<Side> {
        let (width_mm, height_mm) = (self.bounds.width_mm(), self.bounds.height_mm());
        if width_mm < DEGENERATE_MM || height_mm < DEGENERATE_MM {
            return Vec::new();
        }
        let long_enough = |length_mm: f64| length_mm >= camera.px_to_mm(MIN_SIDE_PX);
        Side::ALL
            .into_iter()
            .filter(|side| match side {
                Side::Left | Side::Right => long_enough(height_mm),
                Side::Bottom | Side::Top => long_enough(width_mm),
            })
            .collect()
    }

    /// These handles with the selection's `origin` for their pivot.
    #[must_use]
    pub fn about(mut self, origin: Origin) -> Self {
        self.pivot = origin.point(self.bounds);
        self
    }

    /// The box's centre: the pivot until an origin is chosen.
    #[must_use]
    pub fn centre(&self) -> Point {
        centre(self.bounds)
    }
}

/// The handles of `ids` seen through `camera`; `None` when nothing is
/// selected or the selection has no outline.
#[must_use]
pub fn handles(project: &Project, ids: &[InstanceId], camera: &Camera) -> Option<Handles> {
    let bounds = selection_bounds(project, ids).ok()?;
    let (min, max) = (bounds.min, bounds.max);
    let mid_x = (min.x + max.x) / 2.0;
    Some(Handles {
        bounds,
        corners: [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)],
        rotation: Point::new(mid_x, max.y + camera.px_to_mm(STEM_PX)),
        pivot: centre(bounds),
        locked: project
            .instances
            .iter()
            .any(|i| i.locked && ids.contains(&i.id)),
    })
}

/// The size of `rect`, mm, written under the middle of its bottom edge
/// (functional spec, capability 83a): `40 × 30` in `units`, as a
/// rectangle being drawn shows its own.
#[must_use]
pub fn box_label(strings: &Strings, units: Units, rect: Rect) -> Label {
    Label {
        at_mm: Point::new(f64::midpoint(rect.min.x, rect.max.x), rect.min.y),
        text: strings.fmt(
            StrKey::DrawLabelBox,
            &[
                ("width", &crate::units::summary_text(rect.width_mm(), units)),
                (
                    "height",
                    &crate::units::summary_text(rect.height_mm(), units),
                ),
            ],
        ),
    }
}

/// The size the selection has while a drag of `grab` asks for `delta`
/// (capability 83a): its box as the release would leave it, moved or
/// resized. `None` for a rotation: a box turned is no longer the
/// selection's box, and the status bar reads the angle.
#[must_use]
pub fn size_label(
    strings: &Strings,
    units: Units,
    handles: &Handles,
    grab: Grab,
    delta: Transform,
) -> Option<Label> {
    if matches!(grab, Grab::Rotate { .. }) {
        return None;
    }
    let placed = about_centre(handles, delta);
    let [first, rest @ ..] = handles.corners.map(|corner| placed.apply(corner));
    let rect = rest
        .into_iter()
        .fold(Rect::from_point(first), Rect::including);
    Some(box_label(strings, units, rect))
}

/// Why `ids` cannot be transformed: [`CommandError::Locked`] naming the
/// locked ones, the message the status bar shows; `None` when none is.
#[must_use]
pub fn refusal(project: &Project, ids: &[InstanceId]) -> Option<CommandError> {
    let names: Vec<String> = ids
        .iter()
        .filter_map(|id| project.instances.iter().find(|i| i.id == *id))
        .filter(|i| i.locked)
        .map(|i| i.name.clone())
        .collect();
    (!names.is_empty()).then_some(CommandError::Locked { names })
}

/// What a press at pane point `at` takes hold of: the rotation handle,
/// a corner, the inside of the box (within [`HANDLE_HIT_PX`] of it), or
/// nothing.
#[must_use]
pub fn grab_at(handles: &Handles, camera: &Camera, at: Pos) -> Option<Grab> {
    let near = |p: Point| camera.to_screen(p).distance_to(at) <= HANDLE_HIT_PX;
    if near(handles.rotation) {
        return Some(Grab::Rotate { snap: false });
    }
    if let Some(corner) = Corner::ALL.into_iter().find(|c| near(handles.corner(*c))) {
        return Some(Grab::Corner(corner));
    }
    if let Some(side) = handles
        .sides(camera)
        .into_iter()
        .find(|side| near(handles.side(*side)))
    {
        return Some(Grab::Side(side));
    }
    let p = camera.to_world(at);
    let slack = camera.px_to_mm(HANDLE_HIT_PX);
    let b = handles.bounds;
    (p.x >= b.min.x - slack
        && p.x <= b.max.x + slack
        && p.y >= b.min.y - slack
        && p.y <= b.max.y + slack)
        .then_some(Grab::Move)
}

/// `angle_rad` folded into (−π, π].
fn fold(angle_rad: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    let folded = (angle_rad + PI).rem_euclid(TAU) - PI;
    if approx_eq(folded, -PI, 1e-12) {
        PI
    } else {
        folded
    }
}

/// `to` held level with or upright from `from`: on the horizontal or the
/// vertical through it, whichever `to` has gone further along (level
/// when as far each way). What `Shift` does to a move.
#[must_use]
pub fn straight(from: Point, to: Point) -> Point {
    if (to.x - from.x).abs() >= (to.y - from.y).abs() {
        Point::new(to.x, from.y)
    } else {
        Point::new(from.x, to.y)
    }
}

/// The change a drag of `grab` from `from` to `to` (project mm) asks for,
/// as `TransformInstances` takes it: about the selection's centre. A
/// corner scales by the pointer's distance from the centre over the
/// grabbed corner's, the same on both axes when `uniform` (the ratio
/// along the corner's diagonal), never below [`MIN_SCALE`]; a side
/// stretches one axis, whatever `uniform` says, and keeps the opposite
/// side in place; the rotation
/// handle turns by the angle swept about the centre; a move is the
/// pointer's travel.
#[must_use]
pub fn drag_delta(
    handles: &Handles,
    grab: Grab,
    from: Point,
    to: Point,
    uniform: bool,
) -> Transform {
    // A corner that is the pivot has nothing to pull against there, so
    // it scales from the opposite corner.
    let c = match grab {
        Grab::Corner(corner)
            if handles.corner(corner).distance_to(handles.pivot) < DEGENERATE_MM =>
        {
            let grabbed = handles.corner(corner);
            let middle = handles.centre();
            Point::new(2.0 * middle.x - grabbed.x, 2.0 * middle.y - grabbed.y)
        }
        _ => handles.pivot,
    };
    // The change is made about `c`; the command takes it about the
    // box's centre.
    let placed = |change: Transform| crate::origin::about_point(handles.bounds, c, change);
    let (fx, fy) = (from.x - c.x, from.y - c.y);
    let (tx, ty) = (to.x - c.x, to.y - c.y);
    match grab {
        Grab::Move => Transform::translate(to.x - from.x, to.y - from.y),
        Grab::Corner(_) if uniform => {
            let length_sq = fx * fx + fy * fy;
            if length_sq < DEGENERATE_MM * DEGENERATE_MM {
                return Transform::IDENTITY;
            }
            let s = ((tx * fx + ty * fy) / length_sq).max(MIN_SCALE);
            placed(Transform::scale(s, s))
        }
        Grab::Corner(_) => {
            let ratio = |t: f64, f: f64| {
                if f.abs() < DEGENERATE_MM {
                    1.0
                } else {
                    (t / f).max(MIN_SCALE)
                }
            };
            placed(Transform::scale(ratio(tx, fx), ratio(ty, fy)))
        }
        Grab::Side(side) => {
            // The side goes where the pointer goes along its own axis;
            // scaled about the centre, the box is then moved by half of
            // what it gained, which puts the far side back.
            let b = handles.bounds;
            let (size, travel) = match side {
                Side::Left => (b.width_mm(), from.x - to.x),
                Side::Right => (b.width_mm(), to.x - from.x),
                Side::Bottom => (b.height_mm(), from.y - to.y),
                Side::Top => (b.height_mm(), to.y - from.y),
            };
            if size < DEGENERATE_MM {
                return Transform::IDENTITY;
            }
            let s = ((size + travel) / size).max(MIN_SCALE);
            let half = (s - 1.0) * size / 2.0;
            match side {
                Side::Left => Transform::scale(s, 1.0).then(Transform::translate(-half, 0.0)),
                Side::Right => Transform::scale(s, 1.0).then(Transform::translate(half, 0.0)),
                Side::Bottom => Transform::scale(1.0, s).then(Transform::translate(0.0, -half)),
                Side::Top => Transform::scale(1.0, s).then(Transform::translate(0.0, half)),
            }
        }
        Grab::Rotate { snap } => {
            if fx.hypot(fy) < DEGENERATE_MM || tx.hypot(ty) < DEGENERATE_MM {
                return Transform::IDENTITY;
            }
            let swept = fold(ty.atan2(tx) - fy.atan2(fx));
            let angle = if snap {
                let step = SNAP_DEG.to_radians();
                (swept / step).round() * step
            } else {
                swept
            };
            placed(Transform::rotate(angle))
        }
    }
}

/// `delta` placed about the handles' centre: what to apply after each
/// selected instance's transform to draw it where the command will put
/// it.
#[must_use]
pub fn about_centre(handles: &Handles, delta: Transform) -> Transform {
    about(handles.centre(), delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_fold_into_a_half_turn_either_way() {
        use std::f64::consts::PI;
        assert!(approx_eq(fold(1.5 * PI), -0.5 * PI, 1e-12));
        assert!(approx_eq(fold(-PI), PI, 1e-12));
        assert!(approx_eq(fold(0.25), 0.25, 1e-12));
    }
}
