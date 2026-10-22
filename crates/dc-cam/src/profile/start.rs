// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Where a Profile's cut starts on each closed outline (#285;
//! `docs/specs/phase-2/design.md`, "Phase 2 addendum", A9; functional spec
//! capability 36). VCarve's 2D Profile "Use vector start point" plunges at
//! the shape's first point and "Start At" picks one; Fusion has "Entry
//! Positions". PureCutCNC has neither, so nothing is ported.
//!
//! [`StartAt::FirstVertex`] and [`StartAt::Near`] re-seam each closed
//! tool-centre path at its point nearest the drawn first vertex (placed by
//! the instance's transform) or the given point, before the tabs are laid
//! on it, so a request's `at_distances` count from there and its `count`
//! spaces from there. The nearest point is [`nearest_point`]'s: the first
//! along the path on a tie, except on an arc centred on the target, where
//! the arc's middle is taken, as for placed tabs (ADR-0005), so a corner's
//! first vertex starts the cut in the middle of the arc an outside path
//! turns round it. A last pass's allowance path (#284) is re-seamed nearest
//! the same point. Open paths are left alone: the end their cut starts at
//! is the cut direction's (#242). Nothing here depends on Y-up or Y-down.

use super::CentrePath;
use dc_geometry::{Point, Polyline, nearest_point};
use dc_project::{ProfileOp, Project, StartAt};

use crate::error::ShapeRef;
use crate::lead::reseam_at;

/// `placed`, each closed path (and its allowance twin) re-seamed nearest
/// the start `op` sets, with which of them it set: every closed path for
/// [`StartAt::FirstVertex`] and [`StartAt::Near`], none for
/// [`StartAt::Auto`], which keeps today's paths.
pub(super) fn seat<'s>(
    project: &Project,
    op: &ProfileOp,
    placed: Vec<CentrePath<'s>>,
) -> (Vec<CentrePath<'s>>, Vec<bool>) {
    let mut fixed = Vec::with_capacity(placed.len());
    let seated = placed
        .into_iter()
        .map(|c| {
            let target = c
                .path
                .is_closed()
                .then(|| target(project, op.start, c.shape))
                .flatten();
            fixed.push(target.is_some());
            match target {
                Some(at) => CentrePath {
                    path: seamed(&c.path, at),
                    twin: c.twin.map(|twin| seamed(&twin, at)),
                    ..c
                },
                None => c,
            }
        })
        .collect();
    (seated, fixed)
}

/// The point a closed path of `shape` starts nearest for `start`: the
/// shape's first vertex as drawn, placed by its instance's transform, or
/// the given point. `None` for [`StartAt::Auto`], or when the shape is not
/// in the project.
fn target(project: &Project, start: StartAt, shape: &ShapeRef) -> Option<Point> {
    match start {
        StartAt::Auto => None,
        StartAt::Near { x_mm, y_mm } => Some(Point::new(x_mm, y_mm)),
        StartAt::FirstVertex => {
            let instance = project.instances.iter().find(|i| i.id == shape.instance)?;
            let first = project
                .definitions
                .get(&instance.definition)?
                .profiles
                .get(shape.profile)?
                .vertices()
                .first()?
                .point();
            Some(instance.transform.apply(first))
        }
    }
}

/// The closed `path` re-seamed at its point nearest `at`: the same loop,
/// started there.
fn seamed(path: &Polyline, at: Point) -> Polyline {
    match nearest_point(path, at) {
        Some(near) => reseam_at(path, near.segment, near.along_segment_mm),
        None => path.clone(),
    }
}
