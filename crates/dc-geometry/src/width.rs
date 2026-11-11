// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Widths and gaps of regions (sub-phase 3.4 Image Prep,
//! `docs/specs/phase-3/design.md`, "dc-geometry (sub-phase 3.4)" and "Image
//! prep" steps 5 and 6; slice 21b, #595).
//!
//! Two kinds of question, with two costs. Whether a region or a hole is
//! narrower than a threshold ([`is_narrower_than`],
//! [`hole_is_narrower_than`]) is one erosion: no circle of radius `d / 2`
//! fits exactly when the region inset by `d / 2` is empty, whatever the
//! shape or length. That is what the cleanup steps use. How wide a region
//! is along its length ([`local_widths`]) and how near two parts come
//! ([`narrowest_gap`]) build a medial axis or a search grid, and are for
//! the report, once, on the final pieces.

use crate::carve::{floor_regions, single_region};
use crate::contour::Contour;
use crate::medial::{medial_axis, neck_step_mm, refined_step_mm, resolution};
use crate::merge::{folds_back, outline_pairs};
use crate::point::Point;
use crate::region::Region;

/// How far short of half the width the erosion stops, mm, so a region
/// exactly as wide as the threshold keeps a sliver the offset does not
/// round away: 0.2 µm on the width, far under any design tolerance.
const ERODE_SLACK_MM: f64 = 1e-4;

/// The narrowest gap found by [`narrowest_gap`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gap {
    /// The gap, mm: the least distance between the two outlines.
    pub width_mm: f64,
    /// X of the middle of the gap where it is narrowest, mm.
    pub x_mm: f64,
    /// Y of the middle of the gap where it is narrowest, mm.
    pub y_mm: f64,
}

/// The widths along a region ([`local_widths`]): one sample for each edge
/// of its medial axis, as `(width_mm, length_mm)`, sorted by width.
#[derive(Debug, Clone, PartialEq)]
pub struct WidthProfile {
    samples: Vec<(f64, f64)>,
}

impl WidthProfile {
    /// A profile of `samples`, `(width_mm, length_mm)`, sorted here.
    /// `None`-free: an empty profile has median and minimum 0.
    pub(crate) fn from_samples(mut samples: Vec<(f64, f64)>) -> Self {
        samples.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        Self { samples }
    }

    /// The samples, `(width_mm, length_mm)`, narrowest first.
    #[must_use]
    pub fn samples(&self) -> &[(f64, f64)] {
        &self.samples
    }

    /// The length-weighted median width, mm: the width below which half
    /// of the axis's length lies. 0 for an empty profile.
    #[must_use]
    pub fn median_mm(&self) -> f64 {
        let total_mm: f64 = self.samples.iter().map(|s| s.1).sum();
        let mut below_mm = 0.0;
        for &(width_mm, length_mm) in &self.samples {
            below_mm += length_mm;
            if below_mm >= total_mm / 2.0 {
                return width_mm;
            }
        }
        self.samples.last().map_or(0.0, |s| s.0)
    }

    /// The narrowest width, mm. 0 for an empty profile.
    #[must_use]
    pub fn min_mm(&self) -> f64 {
        self.samples.first().map_or(0.0, |s| s.0)
    }
}

/// Whether the widest point of `region` is under `width_mm`: the diameter
/// of the largest circle that fits in it, inside its outer and outside its
/// holes. Exact by erosion, [`floor_regions`] by half the width, which is
/// empty exactly when no circle of that radius fits; no medial axis, one
/// offset. A region exactly `width_mm` wide is not narrower (the erosion is
/// taken [`ERODE_SLACK_MM`] short of the radius, so rounding cannot empty
/// it).
/// False when `width_mm` is not a positive finite number.
#[must_use]
pub fn is_narrower_than(region: &Region, width_mm: f64, tolerance_mm: f64) -> bool {
    if !(width_mm.is_finite() && width_mm > 0.0) {
        return false;
    }
    let radius_mm = (width_mm / 2.0 - ERODE_SLACK_MM).max(0.0);
    floor_regions(std::slice::from_ref(region), radius_mm, tolerance_mm).is_empty()
}

/// Whether `hole`'s widest point is under `width_mm`: the same test on the
/// hole as a region of its own, the largest circle that fits in the hole.
/// False when `width_mm` is not a positive finite number.
#[must_use]
pub fn hole_is_narrower_than(hole: &Contour, width_mm: f64, tolerance_mm: f64) -> bool {
    // A hole is clockwise inside its region; as a region of its own it is
    // the area it encloses.
    let area = if hole.is_ccw() {
        hole.clone()
    } else {
        hole.reversed()
    };
    match single_region(area) {
        Some(inside) => is_narrower_than(&inside, width_mm, tolerance_mm),
        None => false,
    }
}

/// The width along `region`'s length: for every edge of its medial axis
/// between two nodes that are each joined to two or more others, twice the
/// mean clearance of its ends (the tips at corners, clearance falling to 0,
/// are leaves and are left out), with the edge's length. The axis is
/// sampled at [`resolution`]'s step, and again at the finer step the
/// graph's narrowest bottleneck or neck asks for (#475, #553), so a
/// hairline in a large piece is seen. `None` for a region too small for an
/// axis.
///
/// Along a corner's branch the clearance still falls towards the tip, so
/// [`WidthProfile::min_mm`] is small for any shape with a sharp corner;
/// the report reads the length-weighted median, which those short branches
/// do not move.
#[must_use]
pub fn local_widths(region: &Region, tolerance_mm: f64) -> Option<WidthProfile> {
    let resolved = resolution(region)?;
    let mut graph = medial_axis(region, resolved.step_mm, tolerance_mm);
    if graph.nodes.is_empty() {
        return None;
    }
    let refined_mm = [
        refined_step_mm(&resolved, &graph),
        neck_step_mm(region, &resolved, tolerance_mm),
    ]
    .into_iter()
    .flatten()
    .reduce(f64::min);
    if let Some(step_mm) = refined_mm {
        let finer = medial_axis(region, step_mm, tolerance_mm);
        if !finer.nodes.is_empty() {
            graph = finer;
        }
    }
    let mut samples = Vec::new();
    for (i, adjacent) in graph.adjacency.iter().enumerate() {
        let Some(a) = graph.nodes.get(i) else {
            continue;
        };
        if adjacent.len() < 2 {
            continue;
        }
        for &j in adjacent.iter().filter(|&&j| j > i) {
            let Some(b) = graph.nodes.get(j) else {
                continue;
            };
            if graph.adjacency.get(j).is_none_or(|n| n.len() < 2) {
                continue;
            }
            let length_mm = a.p.distance_to(b.p);
            if length_mm > 0.0 {
                samples.push((a.clearance_mm + b.clearance_mm, length_mm));
            }
        }
    }
    (!samples.is_empty()).then(|| WidthProfile::from_samples(samples))
}

/// The narrowest gap between `regions` (nested by [`crate::nest`]),
/// searching no farther than `reach_mm`: the least distance between two
/// outlines of different regions or, within one region, between two
/// stretches of an outline more than eight gaps apart along it (the pair
/// of a notch's walls). It is [`crate::merge_near_parts`]'s own search with
/// the reach raised from the merge distance to `reach_mm`: the same
/// outline pairs and the same fold test, so a gap is what the merge would
/// close. A gap lies outside the design: touching outlines (within
/// [`EPS_MM`]) and a web of material between two holes are not gaps.
/// `None` when nothing lies within the reach, or the reach is not a
/// positive finite number.
#[must_use]
pub fn narrowest_gap(regions: &[Region], reach_mm: f64, _tolerance_mm: f64) -> Option<Gap> {
    if !(reach_mm.is_finite() && reach_mm > 0.0) {
        return None;
    }
    let mut best: Option<Gap> = None;
    let mut consider = |gap_mm: f64, at: Point| {
        if best.is_none_or(|b| gap_mm < b.width_mm) {
            best = Some(Gap {
                width_mm: gap_mm,
                x_mm: at.x,
                y_mm: at.y,
            });
        }
    };
    for pair in outline_pairs(regions, reach_mm) {
        consider(pair.gap_mm, pair.at);
    }
    for region in regions {
        let outside = |at: Point| !region.contains_point(at);
        for contour in std::iter::once(region.outer()).chain(region.holes()) {
            if let Some((p, q)) = folds_back(contour, reach_mm, &outside, true) {
                consider(
                    p.distance_to(q),
                    Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0),
                );
            }
        }
    }
    best
}

#[cfg(test)]
mod tests;
