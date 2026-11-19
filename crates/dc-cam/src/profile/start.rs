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
//! turns round it. A last pass's stock path (#284) is re-seamed nearest
//! the same point. Open paths are left alone: the end their cut starts at
//! is the cut direction's (#242). Nothing here depends on Y-up or Y-down.

use super::CentrePath;
use dc_geometry::{Point, Polyline, nearest_point};
use dc_project::{ProfileOp, Project, StartAt};

use crate::error::{CamError, ShapeRef};
use crate::lead::reseam_at;

/// `placed`, each closed path (and its stock to leave twin) re-seamed nearest
/// the start `op` sets, with which of them it set: every closed path for
/// [`StartAt::FirstVertex`] and [`StartAt::Near`], none for
/// [`StartAt::Auto`], which keeps today's paths. Fails as [`target`]
/// does.
pub(super) fn seat<'s>(
    project: &Project,
    op: &ProfileOp,
    placed: Vec<CentrePath<'s>>,
) -> Result<(Vec<CentrePath<'s>>, Vec<bool>), CamError> {
    let mut fixed = Vec::with_capacity(placed.len());
    let mut seated = Vec::with_capacity(placed.len());
    for c in placed {
        let target = if c.path.is_closed() {
            target(project, op.start, c.shape)?
        } else {
            None
        };
        fixed.push(target.is_some());
        seated.push(match target {
            Some(at) => CentrePath {
                path: seamed(&c.path, at),
                twin: c.twin.map(|twin| seamed(&twin, at)),
                ..c
            },
            None => c,
        });
    }
    Ok((seated, fixed))
}

/// The point a closed path of `shape` starts nearest for `start`: the
/// shape's first vertex as drawn, placed by its instance's transform, or
/// the given point. `None` for [`StartAt::Auto`]. `shape` is one the
/// Profile's targets were resolved to, so it is in the project with a
/// first vertex: [`CamError::Internal`] otherwise, where the path would
/// keep its own start and the start the user set be ignored.
fn target(project: &Project, start: StartAt, shape: &ShapeRef) -> Result<Option<Point>, CamError> {
    match start {
        StartAt::Auto => Ok(None),
        StartAt::Near { x_mm, y_mm } => Ok(Some(Point::new(x_mm, y_mm))),
        StartAt::FirstVertex => {
            let drawn = project
                .instances
                .iter()
                .find(|i| i.id == shape.instance)
                .and_then(|instance| {
                    let first = project
                        .definitions
                        .get(&instance.definition)?
                        .profiles
                        .get(shape.profile)?
                        .vertices()
                        .first()?;
                    Some(instance.transform.apply(first.point()))
                });
            drawn.map(Some).ok_or_else(|| {
                CamError::internal("a shape a Profile cuts has no first vertex in the project")
            })
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
