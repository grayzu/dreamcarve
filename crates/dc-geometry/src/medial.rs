// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/vcarveMedial/medialAxis.ts
//   src/engine/toolpaths/vcarveMedial/resolution.ts

//! The medial axis of a region (`docs/specs/phase-3/design.md`,
//! "Algorithms → Medial axis (dc-geometry::medial)"): the skeleton a V-bit
//! follows, each node carrying its exact distance to the boundary.

use crate::point::{Point, Vec2};
use crate::polyline::Polyline;
use crate::region::Region;

#[cfg(test)]
mod tests;

/// One node of a [`MedialGraph`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MedialNode {
    /// Position, mm, Y-up.
    pub p: Point,
    /// Exact distance from `p` to the nearest boundary segment, mm; 0 at a
    /// corner tip.
    pub clearance_mm: f64,
}

/// A region's medial axis as an undirected graph.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MedialGraph {
    /// The nodes.
    pub nodes: Vec<MedialNode>,
    /// `adjacency[i]` lists the nodes joined to node `i`, ascending; the
    /// relation is symmetric.
    pub adjacency: Vec<Vec<usize>>,
}

/// The automatic sampling step of a region ([`resolution`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolution {
    /// The step to sample the boundary at, mm.
    pub step_mm: f64,
    /// The sample-budget floor, mm: the perimeter over
    /// [`MEDIAL_SAMPLE_BUDGET`].
    pub budget_floor_mm: f64,
    /// True when the budget floor, not the shape, set the step.
    pub budget_limited: bool,
}

/// A convex corner of a region ([`convex_corners`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    /// The corner vertex, mm.
    pub p: Point,
    /// How far the boundary turns there, radians (0 is straight on).
    pub turn_rad: f64,
    /// Unit vector along the corner's bisector, into the region.
    pub bisector: Vec2,
}

/// Samples across the short side of the outer's bounding box.
pub const MEDIAL_SAMPLES_ACROSS_SHORT_SPAN: f64 = 160.0;
/// Boundary samples a region may have before the step is coarsened.
pub const MEDIAL_SAMPLE_BUDGET: f64 = 40_000.0;
/// The turn, radians, beyond which a convex corner gets a zero-clearance
/// tip: 50°.
pub const CORNER_ANGLE_RAD: f64 = 50.0 * std::f64::consts::PI / 180.0;

/// The sampling step for `region`'s medial axis.
#[must_use]
pub fn resolution(region: &Region) -> Option<Resolution> {
    let holes: Vec<&Polyline> = region.holes().iter().map(|h| h.polyline()).collect();
    resolve(region.outer().polyline(), &holes)
}

fn resolve(_outer: &Polyline, _holes: &[&Polyline]) -> Option<Resolution> {
    None
}

/// The medial axis of `region` sampled at `step_mm`.
#[must_use]
pub fn medial_axis(_region: &Region, _step_mm: f64, _tolerance_mm: f64) -> MedialGraph {
    MedialGraph::default()
}

fn medial_axis_of_loops(_loops: &[Vec<Point>], _step_mm: f64) -> MedialGraph {
    MedialGraph::default()
}

/// The convex corners of `region` sharper than `min_turn_rad`.
#[must_use]
pub fn convex_corners(_region: &Region, _min_turn_rad: f64, _tolerance_mm: f64) -> Vec<Corner> {
    Vec::new()
}

/// Even-odd point-in-polygon test over `loops`.
#[must_use]
pub fn point_in_loops(_p: Point, _loops: &[&Polyline]) -> bool {
    false
}
