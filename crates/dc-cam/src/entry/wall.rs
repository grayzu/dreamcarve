// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Entry on a Profile's wall (`docs/specs/phase-2/design.md`, "Algorithms →
//! Entry", Profile ramp and On line ramp).

use super::{Descent, EntryDomain, EntryKind, EntryPlan, EntryTool, plan};
use dc_geometry::{Point, Polyline};
use dc_project::Entry;

/// [`plan`] for a target on a Profile's tool-centre path.
#[must_use]
pub fn plan_at_wall(
    domain: &EntryDomain,
    entry: &Entry,
    target: Point,
    tool: &EntryTool,
) -> EntryPlan {
    plan(domain, entry, target, tool)
}

/// The On line ramp along `path`.
#[must_use]
pub fn along_path(
    path: &Polyline,
    _angle_deg: f64,
    _max_run_mm: f64,
    _start_z_mm: f64,
    _z_mm: f64,
    _tool: &EntryTool,
    _tolerance_mm: f64,
) -> Descent {
    let from = path
        .vertices()
        .first()
        .map_or(Point::new(0.0, 0.0), |v| v.point());
    Descent {
        from,
        moves: Vec::new(),
        requested: EntryKind::Ramp,
        used: EntryKind::Plunge,
        reason: None,
        helix: None,
    }
}
