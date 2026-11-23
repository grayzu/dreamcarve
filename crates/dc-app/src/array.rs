// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/sketch/featureDistribution.ts

//! Arrays (functional spec, capability 98): where the copies of a grid
//! and of a circular array go, and the command that makes them
//! (`planFeatureDistribution`'s grid and radial modes).
//!
//! As in PureCutCNC the source is the array's first cell and stays where
//! it is; a circular array starts at the source and, round a whole turn,
//! does not lay its last copy on it. Where it leaves PureCutCNC: a grid's
//! spacing can be the gap between copies as well as their pitch; a
//! positive angle turns anticlockwise (ADR-0001: Y-up); the copies are
//! always linked and arrive in one group with their source; and a taper
//! of sizes and an array along a path are not ported.

use dc_geometry::{Point, Rect, Transform};
use dc_project::{InstanceId, Project};

use crate::command::{Command, find_instance, selection_bounds};
use crate::error::CommandError;
use crate::strings::{StrKey, Strings};

/// The most copies one array makes.
pub const MAX_COPIES: usize = 1000;

/// A whole turn, degrees.
const FULL_TURN_DEG: f64 = 360.0;
/// Two copies nearer than this, mm, lie on one another.
const APART_MM: f64 = 1e-6;
/// A total angle within this of a whole turn is one, degrees.
const WHOLE_DEG: f64 = 1e-6;

/// What a grid's spacing measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSpacing {
    /// From one copy's side to the next copy's near side.
    Gap,
    /// From one copy's side to the next copy's same side.
    Pitch,
}

/// A grid of copies: the source is its first cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// How many cells along X.
    pub columns: u32,
    /// How many cells along Y.
    pub rows: u32,
    /// The spacing along X, mm; below nothing goes toward −X.
    pub x_mm: f64,
    /// The spacing along Y, mm; below nothing goes toward −Y.
    pub y_mm: f64,
    /// What the spacings measure.
    pub spacing: GridSpacing,
}

/// Copies round a centre: the source is the first of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Circular {
    /// How many there are, the source among them.
    pub count: u32,
    /// What they go round, project space, mm.
    pub centre: Point,
    /// The angle they are spread over, degrees, positive anticlockwise.
    pub total_deg: f64,
    /// Whether each copy is turned as it goes round, or kept upright.
    pub turn: bool,
}

/// An array to make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Array {
    /// Rows and columns.
    Grid(Grid),
    /// Round a centre.
    Circular(Circular),
}

/// Why an array cannot be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ArrayError {
    /// A grid of one cell.
    #[error("an array needs two copies or more: set the columns or the rows")]
    OneCell,
    /// A circular array of one.
    #[error("an array needs two copies or more")]
    OneCopy,
    /// More than [`MAX_COPIES`].
    #[error("an array makes at most 1000 copies")]
    TooMany,
    /// A spacing that moves a copy nowhere.
    #[error("the copies would lie on one another: set a spacing")]
    NoSpacing,
    /// An angle that moves a copy nowhere.
    #[error("the copies would lie on one another: set an angle")]
    NoAngle,
    /// The centre is the middle of what is copied.
    #[error("the centre is the selection's own middle: choose another")]
    CentreOnSelection,
    /// A circular array with no centre yet.
    #[error("click the centre of the circle, or type it")]
    NoCentre,
    /// A size that is no number.
    #[error("an array's sizes must be numbers")]
    NotFinite,
}

/// Where each copy of what fills `bounds` goes: one transform a copy,
/// applied after the source's own, the source's cell left out. A grid's
/// copies are in rows from the source's, each from the source's column.
///
/// # Errors
///
/// [`ArrayError`] for an array that is no array.
pub fn placements(array: &Array, bounds: Rect) -> Result<Vec<Transform>, ArrayError> {
    match array {
        Array::Grid(grid) => grid_placements(grid, bounds),
        Array::Circular(circular) => circular_placements(circular, bounds),
    }
}

fn grid_placements(grid: &Grid, bounds: Rect) -> Result<Vec<Transform>, ArrayError> {
    if !grid.x_mm.is_finite() || !grid.y_mm.is_finite() {
        return Err(ArrayError::NotFinite);
    }
    let cells = u64::from(grid.columns) * u64::from(grid.rows);
    if cells < 2 {
        return Err(ArrayError::OneCell);
    }
    if cells > MAX_COPIES as u64 {
        return Err(ArrayError::TooMany);
    }
    let (pitch_x, pitch_y) = match grid.spacing {
        GridSpacing::Pitch => (grid.x_mm, grid.y_mm),
        GridSpacing::Gap => (
            bounds.width_mm() + grid.x_mm,
            bounds.height_mm() + grid.y_mm,
        ),
    };
    if (grid.columns > 1 && pitch_x.abs() < APART_MM) || (grid.rows > 1 && pitch_y.abs() < APART_MM)
    {
        return Err(ArrayError::NoSpacing);
    }
    let mut out = Vec::new();
    for row in 0..grid.rows {
        for column in 0..grid.columns {
            if row == 0 && column == 0 {
                continue;
            }
            out.push(Transform::translate(
                f64::from(column) * pitch_x,
                f64::from(row) * pitch_y,
            ));
        }
    }
    Ok(out)
}

fn circular_placements(circular: &Circular, bounds: Rect) -> Result<Vec<Transform>, ArrayError> {
    let centre = circular.centre;
    if !centre.x.is_finite() || !centre.y.is_finite() || !circular.total_deg.is_finite() {
        return Err(ArrayError::NotFinite);
    }
    if circular.count < 2 {
        return Err(ArrayError::OneCopy);
    }
    if circular.count as usize > MAX_COPIES {
        return Err(ArrayError::TooMany);
    }
    let pivot = Point::new(
        f64::midpoint(bounds.min.x, bounds.max.x),
        f64::midpoint(bounds.min.y, bounds.max.y),
    );
    if (pivot.x - centre.x).hypot(pivot.y - centre.y) < APART_MM {
        return Err(ArrayError::CentreOnSelection);
    }
    // Round a whole turn the last copy stops one step short of the
    // source; over less, it is at the angle's end.
    let whole = (circular.total_deg.abs() - FULL_TURN_DEG).abs() < WHOLE_DEG;
    let steps = if whole {
        circular.count
    } else {
        circular.count.saturating_sub(1)
    };
    let step_deg = circular.total_deg / f64::from(steps);
    if step_deg.abs() < WHOLE_DEG {
        return Err(ArrayError::NoAngle);
    }
    let about_centre = |turn: Transform| {
        Transform::translate(-centre.x, -centre.y)
            .then(turn)
            .then(Transform::translate(centre.x, centre.y))
    };
    Ok((1..circular.count)
        .map(|copy| {
            let turned = about_centre(Transform::rotate((step_deg * f64::from(copy)).to_radians()));
            if circular.turn {
                turned
            } else {
                let at = turned.apply(pivot);
                Transform::translate(at.x - pivot.x, at.y - pivot.y)
            }
        })
        .collect())
}

/// The command that makes `array` of the instances `ids`: linked copies,
/// in one new group named for the first of them, which their sources
/// join.
///
/// # Errors
///
/// [`CommandError::NothingSelected`] with no instance, the instances'
/// [`CommandError`], or [`CommandError::Array`].
pub fn command(
    project: &Project,
    strings: &Strings,
    ids: &[InstanceId],
    array: &Array,
) -> Result<Command, CommandError> {
    let Some(first) = ids.first() else {
        return Err(CommandError::NothingSelected);
    };
    let bounds = selection_bounds(project, ids)?;
    let placements = placements(array, bounds)?;
    let name = find_instance(project, first)?.name.as_str();
    Ok(Command::CopyInstances {
        ids: ids.to_vec(),
        placements,
        folder: Some(strings.fmt(StrKey::ArrayName, &[("name", name)])),
    })
}
