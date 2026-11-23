// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The origin of a selection (the maintainer's review of 2026-11-23): the
//! point of its box that changes to it are made about. A typed width or
//! height, a turn, a flip, and a drag of a corner or of the rotation
//! handle all hold it still.
//!
//! It is one of nine points of the selection's box, its corners, the
//! middles of its sides and its centre, and the centre until another is
//! chosen: every such change was about the centre before there was a
//! choice. It is a preference of the app ([`crate::Prefs::origin`]), not
//! project data, and it is not where X and Y are measured to: those stay
//! the box's bottom-left corner.
//!
//! `Command::TransformInstances` applies its change about the box's
//! centre. A change about another point is that change followed by the
//! move that puts the point back ([`Origin::about`]), so no command
//! needed to learn of origins.

use dc_geometry::{Point, Rect, Transform};

use crate::command::about;
use crate::strings::StrKey;

/// Which point of a selection's box changes are made about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Origin {
    /// Least X, greatest Y.
    TopLeft,
    /// The middle of the top side.
    Top,
    /// Greatest X, greatest Y.
    TopRight,
    /// The middle of the left side.
    Left,
    /// The box's centre: what every change was about before there was a
    /// choice.
    #[default]
    Centre,
    /// The middle of the right side.
    Right,
    /// Least X, least Y.
    BottomLeft,
    /// The middle of the bottom side.
    Bottom,
    /// Greatest X, least Y.
    BottomRight,
}

impl Origin {
    /// The nine, as the chooser lays them out and as the box is seen:
    /// rows from the top (the greatest Y), each from the left.
    pub const ALL: [Self; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Centre,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    /// Where it is on `bounds`, mm, Y-up.
    #[must_use]
    pub fn point(self, bounds: Rect) -> Point {
        let (min, max) = (bounds.min, bounds.max);
        let x = match self {
            Self::TopLeft | Self::Left | Self::BottomLeft => min.x,
            Self::Top | Self::Centre | Self::Bottom => f64::midpoint(min.x, max.x),
            Self::TopRight | Self::Right | Self::BottomRight => max.x,
        };
        let y = match self {
            Self::BottomLeft | Self::Bottom | Self::BottomRight => min.y,
            Self::Left | Self::Centre | Self::Right => f64::midpoint(min.y, max.y),
            Self::TopLeft | Self::Top | Self::TopRight => max.y,
        };
        Point::new(x, y)
    }

    /// Its name, for the chooser's buttons.
    #[must_use]
    pub const fn name(self) -> StrKey {
        match self {
            Self::TopLeft => StrKey::OriginTopLeft,
            Self::Top => StrKey::OriginTop,
            Self::TopRight => StrKey::OriginTopRight,
            Self::Left => StrKey::OriginLeft,
            Self::Centre => StrKey::OriginCentre,
            Self::Right => StrKey::OriginRight,
            Self::BottomLeft => StrKey::OriginBottomLeft,
            Self::Bottom => StrKey::OriginBottom,
            Self::BottomRight => StrKey::OriginBottomRight,
        }
    }

    /// `change`, a scale, a turn or a mirror, made about this point of
    /// `bounds`, as the delta `TransformInstances` takes for a selection
    /// whose box is `bounds`: that command applies its delta about the
    /// box's centre, so the point is given from there.
    #[must_use]
    pub fn about(self, bounds: Rect, change: Transform) -> Transform {
        about_point(bounds, self.point(bounds), change)
    }
}

/// `change` made about `pivot`, as the delta `TransformInstances` takes
/// for a selection whose box is `bounds`.
#[must_use]
pub fn about_point(bounds: Rect, pivot: Point, change: Transform) -> Transform {
    let centre = Point::new(
        f64::midpoint(bounds.min.x, bounds.max.x),
        f64::midpoint(bounds.min.y, bounds.max.y),
    );
    about(Point::new(pivot.x - centre.x, pivot.y - centre.y), change)
}
