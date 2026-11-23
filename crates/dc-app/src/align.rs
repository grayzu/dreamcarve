// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/slices/featureSlice.ts

//! Align and distribute (functional spec, capability 98a): one
//! translation per shape, from the shapes' boxes as they are placed
//! (`alignFeatures`, `distributeFeatures`).
//!
//! Where it leaves PureCutCNC: Top is the greatest Y and Bottom the
//! least (ADR-0001; upstream is Y-down); shapes can align to the stock;
//! and a locked shape, which does not move, still counts toward the box
//! the others align to, so a shape can be aligned to one that is locked.
//! Distribute spaces the unlocked shapes among themselves, as upstream
//! does.

use dc_geometry::{Rect, Transform};
use dc_project::{InstanceId, Project};

use crate::command::selection_bounds;

/// The side of their boxes shapes are aligned by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// The least X.
    Left,
    /// The middle along X.
    Centre,
    /// The greatest X.
    Right,
    /// The greatest Y.
    Top,
    /// The middle along Y.
    Middle,
    /// The least Y.
    Bottom,
}

/// The direction shapes are distributed along.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Along X.
    Horizontal,
    /// Along Y.
    Vertical,
}

/// What Distribute makes equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    /// The gaps between one shape's box and the next.
    Gaps,
    /// The distances between the boxes' middles.
    Centres,
}

/// A change smaller than this, mm, is none: a shape already in place is
/// not moved by rounding.
const STILL_MM: f64 = 1e-9;

/// `delta`, or nothing at all when it is less than [`STILL_MM`].
fn settled(delta: f64) -> f64 {
    if delta.abs() > STILL_MM { delta } else { 0.0 }
}

/// Each of `ids` with the box it fills as placed; one with no outline
/// is left out.
fn boxes(project: &Project, ids: &[InstanceId]) -> Vec<(InstanceId, Rect)> {
    ids.iter()
        .filter_map(|id| {
            let bounds = selection_bounds(project, std::slice::from_ref(id)).ok()?;
            Some((id.clone(), bounds))
        })
        .collect()
}

/// The moves that align `movable` by `side` to `reference`, one for
/// each: no move at all for a shape already there.
fn to_box(
    project: &Project,
    movable: &[InstanceId],
    side: Align,
    reference: Rect,
) -> Vec<(InstanceId, Transform)> {
    boxes(project, movable)
        .into_iter()
        .map(|(id, bounds)| {
            let (dx, dy) = match side {
                Align::Left => (reference.min.x - bounds.min.x, 0.0),
                Align::Right => (reference.max.x - bounds.max.x, 0.0),
                Align::Centre => (
                    f64::midpoint(reference.min.x, reference.max.x)
                        - f64::midpoint(bounds.min.x, bounds.max.x),
                    0.0,
                ),
                Align::Bottom => (0.0, reference.min.y - bounds.min.y),
                Align::Top => (0.0, reference.max.y - bounds.max.y),
                Align::Middle => (
                    0.0,
                    f64::midpoint(reference.min.y, reference.max.y)
                        - f64::midpoint(bounds.min.y, bounds.max.y),
                ),
            };
            (id, Transform::translate(settled(dx), settled(dy)))
        })
        .collect()
}

/// The moves that align `movable` by `side`: to the stock when
/// `to_stock` or when `selected` is one shape, else to the joint box of
/// `selected`, which holds the locked shapes too.
pub(crate) fn align(
    project: &Project,
    selected: &[InstanceId],
    movable: &[InstanceId],
    side: Align,
    to_stock: bool,
) -> Vec<(InstanceId, Transform)> {
    let stock = Rect::from_point(dc_geometry::Point::new(0.0, 0.0)).including(
        dc_geometry::Point::new(project.stock.width_mm, project.stock.height_mm),
    );
    let reference = if to_stock || selected.len() < 2 {
        Some(stock)
    } else {
        selection_bounds(project, selected).ok()
    };
    reference.map_or_else(Vec::new, |reference| {
        to_box(project, movable, side, reference)
    })
}

/// The moves that space `movable` evenly along `axis` between the two
/// outermost, which stay: one for each shape, no move at all for one in
/// place already; none for fewer than three.
pub(crate) fn distribute(
    project: &Project,
    movable: &[InstanceId],
    axis: Axis,
    spacing: Spacing,
) -> Vec<(InstanceId, Transform)> {
    struct Entry {
        id: InstanceId,
        min: f64,
        max: f64,
    }
    impl Entry {
        fn centre(&self) -> f64 {
            f64::midpoint(self.min, self.max)
        }
    }
    let mut entries: Vec<Entry> = boxes(project, movable)
        .into_iter()
        .map(|(id, bounds)| match axis {
            Axis::Horizontal => Entry {
                id,
                min: bounds.min.x,
                max: bounds.max.x,
            },
            Axis::Vertical => Entry {
                id,
                min: bounds.min.y,
                max: bounds.max.y,
            },
        })
        .collect();
    let (Some(count), true) = (entries.len().checked_sub(1), entries.len() >= 3) else {
        return Vec::new();
    };
    match spacing {
        Spacing::Centres => entries.sort_by(|a, b| a.centre().total_cmp(&b.centre())),
        Spacing::Gaps => entries.sort_by(|a, b| a.min.total_cmp(&b.min)),
    }
    let (Some(first), Some(last)) = (entries.first(), entries.last()) else {
        return Vec::new();
    };
    #[allow(clippy::cast_precision_loss, reason = "a count of shapes")]
    let steps = count as f64;
    // Where each shape's least side goes, in order.
    let mut targets = Vec::with_capacity(entries.len());
    match spacing {
        Spacing::Centres => {
            let step = (last.centre() - first.centre()) / steps;
            for (index, entry) in entries.iter().enumerate() {
                #[allow(clippy::cast_precision_loss, reason = "a count of shapes")]
                let centre = step.mul_add(index as f64, first.centre());
                targets.push(centre - (entry.max - entry.min) / 2.0);
            }
        }
        Spacing::Gaps => {
            let sizes: f64 = entries.iter().map(|entry| entry.max - entry.min).sum();
            let gap = (last.max - first.min - sizes) / steps;
            let mut cursor = first.min;
            for entry in &entries {
                targets.push(cursor);
                cursor += entry.max - entry.min + gap;
            }
        }
    }
    entries
        .iter()
        .zip(&targets)
        .enumerate()
        .map(|(index, (entry, target))| {
            // The outermost two stay where they are.
            let delta = if index == 0 || index == count {
                0.0
            } else {
                settled(target - entry.min)
            };
            let moved = match axis {
                Axis::Horizontal => Transform::translate(delta, 0.0),
                Axis::Vertical => Transform::translate(0.0, delta),
            };
            (entry.id.clone(), moved)
        })
        .collect()
}
