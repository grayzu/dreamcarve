// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/vcarve.ts (generateVCarveToolpathSingle: the
//   inset rings at `currentDepth · slope`, sortContoursNearestNeighbor)

//! The offset strategy of a V-Carve (`docs/specs/phase-3/design.md`,
//! "Algorithms → V-Carve generator", step 8; functional 43): ring `k` is
//! the region inset by `k · step_over_mm`, cut at the depth where the
//! bit's flank touches the outline, `inset / tan(α/2)`, until the inset is
//! empty or would reach the flat depth `F`. The ring at `F` is the floor's
//! boundary trace, which the `floor` module cuts with the V-bit floor
//! inside it (step 6).
//!
//! What changed in the port:
//!
//! - **Insets.** Upstream insets each level from the one before
//!   (`buildInsetRegions` with `jtRound`); here every level is the region
//!   itself inset by its own distance ([`floor_regions`], round joins), the
//!   same set without the error adding up. A level that is noise at the
//!   flatten tolerance (Pocket's rule) is not a ring.
//! - **The flat depth.** Upstream stops at the last step at or above its
//!   `maxCarveDepth`. Here the rings stop short of `F · slope`, and the
//!   ring at `F` exactly is the floor's boundary trace (`floor::blocks`),
//!   cut after the V-bit floor it encloses.
//! - **Direction.** Upstream's `applyContourDirection` (Y-down). The rings
//!   run as a Pocket's ([`ring_is_ccw`], re-derived for Y-up in phase 2):
//!   a ring of an outer wall is a Pocket wall ring, a ring round a hole an
//!   island ring.
//! - **Links (security review H2).** Upstream bridges two levels tool-down
//!   within 8 × the step-over on the surface. That budget is dropped: a
//!   link stays down only when the straight move, from the previous ring's
//!   end at its Z to the next ring's start at its Z, keeps the cone inside
//!   the walls all along it ([`Walls::least_room`]); otherwise the tool
//!   retracts. Every point of a ring lies on the final V surface (its cone
//!   touches the wall), so a link is held to the surface the rings cut, not
//!   to a margin inside it: no point of it below the surface by more than
//!   [`EPS_MM`]. On a straight wall the link from a corner to the next
//!   ring's corner runs down the corner's bisector, which is the surface;
//!   anywhere else it would be below it, and the tool retracts.
//! - **Arcs.** The rings keep `offset`'s arcs until they are cut; a V-bit
//!   carve is straight moves, so each arc becomes chords, on the far side
//!   of the arc from its wall (`floor::flatten_inside`, as the floor's
//!   rings): the tip never comes nearer a wall than its ring.
//! - **Blocks.** A run of rings the tool cuts without lifting is one
//!   [`CutBlock`](crate::CutBlock) (as a run of medial chains, #404), entered by a plunge
//!   to its first ring's Z; `sequence` retracts between blocks.

use crate::pocket::rings::is_noise;
use crate::pocket::{MAX_RINGS, RingWall, ring_is_ccw};
use crate::sequence::nearest_of;
use crate::vcarve::cone::Walls;
use crate::vcarve::floor::flatten_inside;
use crate::vcarve::medial::{Depth, Run, Step};
use dc_geometry::{Contour, EPS_MM, Point, Point3, Region, floor_regions};
use dc_project::CutDirection;

/// One level of rings: the region inset by one distance, at one Z.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Level {
    /// The tip's Z, mm.
    pub z_mm: f64,
    /// Each ring's loop and the wall it runs along.
    pub rings: Vec<(RingWall, Contour)>,
}

/// The levels of `region` stepping in by `step_over_mm` (mm, positive)
/// for `depth`, outermost first, outlines flattened within `tolerance_mm`
/// where the insets need it, until the inset is empty or reaches the flat
/// depth's `F · slope` (within [`EPS_MM`]): that ring is the floor's.
/// `None` when the region would have more than [`MAX_RINGS`] rings.
pub(crate) fn levels(
    region: &Region,
    step_over_mm: f64,
    depth: &Depth,
    tolerance_mm: f64,
) -> Option<Vec<Level>> {
    let floor_inset_mm = depth.flat_depth_mm.map(|f| f * depth.slope);
    let mut out = Vec::new();
    let mut rings = 0usize;
    for k in 1..=u32::try_from(MAX_RINGS)
        .unwrap_or(u32::MAX)
        .saturating_add(1)
    {
        let inset_mm = f64::from(k) * step_over_mm;
        if floor_inset_mm.is_some_and(|f| inset_mm >= f - EPS_MM) {
            return Some(out);
        }
        let pieces: Vec<Region> =
            floor_regions(std::slice::from_ref(region), inset_mm, tolerance_mm)
                .into_iter()
                .filter(|r| !is_noise(r, tolerance_mm))
                .collect();
        if pieces.is_empty() {
            return Some(out);
        }
        let level_rings: Vec<(RingWall, Contour)> = pieces
            .iter()
            .flat_map(|piece| {
                std::iter::once((RingWall::Pocket, piece.outer().clone())).chain(
                    piece
                        .holes()
                        .iter()
                        .map(|hole| (RingWall::Island, hole.clone())),
                )
            })
            .collect();
        rings += level_rings.len();
        if rings > MAX_RINGS {
            return None;
        }
        out.push(Level {
            z_mm: depth.start_plane_z_mm - inset_mm / depth.slope,
            rings: level_rings,
        });
    }
    None
}

/// The runs that cut `levels`, outermost first: within a level, each ring
/// started at its point nearest the previous ring's end (upstream's
/// `sortContoursNearestNeighbor`), the first ring of all at its first
/// point; each ring run the way [`ring_is_ccw`] gives for its wall and
/// `direction`; joined to the ring before it tool-down when
/// [`ring_links_tool_down`] passes, else a new run. Arcs are lines within
/// `tolerance_mm` on the far side from their wall ([`ring_points`]).
pub(crate) fn runs(
    levels: &[Level],
    direction: CutDirection,
    walls: &Walls,
    depth: &Depth,
    tolerance_mm: f64,
) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut current: Option<Run> = None;
    let mut position: Option<Point3> = None;
    for level in levels {
        let mut left: Vec<Vec<Point>> = level
            .rings
            .iter()
            .map(|(wall, contour)| {
                let mut points = ring_points(contour, tolerance_mm);
                if contour.is_ccw() != ring_is_ccw(*wall, direction) {
                    points.reverse();
                }
                points
            })
            .filter(|points| points.len() >= 2)
            .collect();
        while !left.is_empty() {
            let (slot, start) = position.map_or((0, 0), |p| nearest_start(&left, p.xy()));
            let mut ring = left.remove(slot);
            let last = ring.len().saturating_sub(1);
            ring.rotate_left(start.min(last));
            let Some(&first) = ring.first() else {
                continue;
            };
            let entry = Point3::new(first.x, first.y, level.z_mm);
            let links = current.is_some()
                && position.is_some_and(|p| ring_links_tool_down(walls, depth, p, entry));
            if !links {
                runs.extend(current.take());
                current = Some(Run {
                    start: entry,
                    steps: Vec::new(),
                });
            }
            let Some(run) = current.as_mut() else {
                continue;
            };
            if links {
                run.steps.push(Step {
                    to: entry,
                    link: true,
                });
            }
            run.steps.extend(
                ring.iter()
                    .skip(1)
                    .chain(std::iter::once(&first))
                    .map(|q| Step {
                        to: Point3::new(q.x, q.y, level.z_mm),
                        link: false,
                    }),
            );
            position = Some(entry);
        }
    }
    runs.extend(current);
    runs
}

/// Security review H2 for a link between two rings: the straight move
/// from `from` to `to`, each at its own Z, stays down only when neither
/// end is above the start plane and the cone along it is nowhere below
/// the final V surface by more than [`EPS_MM`] ([`Walls::least_room`]).
/// A ring's own points have no room to spare (its cone touches the wall),
/// so the margin a medial link keeps ([`super::medial::links_tool_down`])
/// would retract every ring; this holds the link to the surface the rings
/// themselves cut.
pub(crate) fn ring_links_tool_down(walls: &Walls, depth: &Depth, from: Point3, to: Point3) -> bool {
    let top_mm = depth.start_plane_z_mm + EPS_MM;
    from.z <= top_mm
        && to.z <= top_mm
        && from.xy().distance_to(to.xy()) > EPS_MM
        && walls
            .least_room(from, to, depth.start_plane_z_mm, depth.slope)
            .0
            >= -EPS_MM
}

/// The ring of `left` and its point nearest `p`, as `(slot, index)`:
/// nearest first by [`nearest_of`]'s rule, so ties keep the rings' and
/// points' order.
fn nearest_start(left: &[Vec<Point>], p: Point) -> (usize, usize) {
    let candidates = left.iter().enumerate().flat_map(|(slot, ring)| {
        ring.iter()
            .enumerate()
            .map(move |(index, q)| ((slot, index), q.distance_to(p)))
    });
    nearest_of(candidates).unwrap_or((0, 0))
}

/// The points of the loop `contour` (its inside on the left of travel, as
/// [`floor_regions`] winds them: outers counter-clockwise, holes
/// clockwise), every arc replaced by lines within `tolerance_mm` on its
/// inside, away from its wall ([`flatten_inside`]); the first point is not
/// repeated at the end.
fn ring_points(contour: &Contour, tolerance_mm: f64) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    for p in flatten_inside(contour.polyline(), tolerance_mm) {
        if out.last().is_none_or(|q| q.distance_to(p) > EPS_MM) {
            out.push(p);
        }
    }
    while out.len() >= 2
        && out
            .first()
            .zip(out.last())
            .is_some_and(|(a, b)| a.distance_to(*b) <= EPS_MM)
    {
        out.pop();
    }
    out
}
