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
use dc_project::{InstanceId, Project};

use crate::command::{about, centre, selection_bounds};
use crate::error::CommandError;
use crate::viewport::camera::{Camera, Pos};

/// The rotation handle's stem above the box's top edge, logical pixels.
pub const STEM_PX: f64 = 20.0;
/// How far from a handle's centre a press still grabs it, logical pixels:
/// the 8-px square's half plus a little slack.
pub const HANDLE_HIT_PX: f64 = 6.0;
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

/// What a press on the Viewport took hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grab {
    /// The selection itself: a drag moves it.
    Move,
    /// A corner handle: a drag scales about the centre.
    Corner(Corner),
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

    /// The box's centre: the pivot of every scale and turn.
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
        locked: project
            .instances
            .iter()
            .any(|i| i.locked && ids.contains(&i.id)),
    })
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

/// The change a drag of `grab` from `from` to `to` (project mm) asks for,
/// as `TransformInstances` takes it: about the selection's centre. A
/// corner scales by the pointer's distance from the centre over the
/// grabbed corner's, the same on both axes when `uniform` (the ratio
/// along the corner's diagonal), never below [`MIN_SCALE`]; the rotation
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
    let c = handles.centre();
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
            Transform::scale(s, s)
        }
        Grab::Corner(_) => {
            let ratio = |t: f64, f: f64| {
                if f.abs() < DEGENERATE_MM {
                    1.0
                } else {
                    (t / f).max(MIN_SCALE)
                }
            };
            Transform::scale(ratio(tx, fx), ratio(ty, fy))
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
            Transform::rotate(angle)
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
