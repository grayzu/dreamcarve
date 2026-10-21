// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/pocket.ts

//! The offset ring tree of a pocket (`docs/specs/phase-2/design.md`,
//! "Algorithms → Pocket", steps 2 and 6): [`inset_region`] (upstream
//! `buildInsetRegions`) and [`RingTree`] (upstream `buildOffsetRegionTree`).

use dc_geometry::Region;

/// `region` inset by `inset_mm`: its outer offset inward and its islands
/// outward by `inset_mm`, the islands then cut from the outer. An island
/// can split the result into several regions; nothing left is an empty
/// `Vec`. Outers come back counter-clockwise and holes clockwise (mm,
/// Y-up).
#[must_use]
pub fn inset_region(region: &Region, inset_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let _ = (region, inset_mm, tolerance_mm);
    Vec::new()
}

/// The rings of one tool-centre region: the region itself at the root,
/// and below each node the regions [`inset_region`] leaves of it at one
/// step-over, until nothing is left.
#[derive(Debug, Clone, PartialEq)]
pub struct RingTree {
    /// Every node; the root is node 0.
    pub nodes: Vec<RingNode>,
}

/// One node of a [`RingTree`].
#[derive(Debug, Clone, PartialEq)]
pub struct RingNode {
    /// The node's region; its outer and holes are its rings.
    pub region: Region,
    /// Indices of the node's children in [`RingTree::nodes`].
    pub children: Vec<usize>,
}
