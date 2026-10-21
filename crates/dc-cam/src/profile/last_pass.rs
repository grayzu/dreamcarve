// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A Profile's last pass allowance (#284; `docs/specs/phase-2/design.md`,
//! "Phase 2 addendum (#73)", A8; VCarve's 2D Profile "Last Pass
//! Allowance", Fusion's 2D Contour stock to leave and finishing pass).
//! PureCutCNC has no counterpart: it leaves stock with a separate rough
//! operation, so this is DreamCarve's own.
//!
//! Every pass but the last runs on the allowance path, `r + a` from the
//! part on the waste side, and the last on the part's path at `r`. Each
//! allowance path is the same offset as the part's path with the larger
//! distance; this module makes it, refuses a drawing it cannot follow,
//! and carries the part path's tabs onto it.

use super::{Offset, check_band, region_shapes};
use crate::error::{CamError, ShapeRef};
use crate::overlap::within;
use crate::passes::pass_z_mm;
use crate::tabs::PathTabs;
use dc_geometry::{
    Contour, EPS_MM, Point, Polyline, Region, Vertex, distance_mm, nearest_point, offset,
};
use dc_project::{LastPass, ProfileOp, Tool};

/// Refuses a last pass the generator cannot cut (validation reports each
/// first): one on a side with no wall to leave material on
/// ([`CamError::LastPassNeedsSide`]), an allowance that is not more than
/// 0 and at most a third of the tool's `diameter_mm`, or a thickness that
/// is not more than 0 and at most `depth_mm`
/// ([`CamError::InvalidLastPass`]).
pub(super) fn check(
    op: &ProfileOp,
    last_pass: &LastPass,
    diameter_mm: f64,
    depth_mm: f64,
) -> Result<(), CamError> {
    if !op.side.has_waste_area() {
        return Err(CamError::LastPassNeedsSide { op: op.id.clone() });
    }
    let limit_mm = diameter_mm / 3.0;
    let allowance_mm = last_pass.allowance_mm;
    if !(allowance_mm.is_finite() && allowance_mm > 0.0 && allowance_mm <= limit_mm) {
        return Err(CamError::InvalidLastPass {
            op: op.id.clone(),
            field: "allowance_mm",
            value: allowance_mm,
            range: format!(
                "more than 0 and at most {limit_mm:.3} mm, a third of the {diameter_mm} mm tool"
            ),
        });
    }
    if let Some(thickness_mm) = last_pass.thickness_mm
        && !(thickness_mm.is_finite() && thickness_mm > 0.0 && thickness_mm <= depth_mm + EPS_MM)
    {
        return Err(CamError::InvalidLastPass {
            op: op.id.clone(),
            field: "thickness_mm",
            value: thickness_mm,
            range: format!("more than 0 and at most the {depth_mm} mm depth"),
        });
    }
    Ok(())
}

/// The pass Zs of a cut `depth_mm` deep into stock `thickness_mm` thick
/// at `step_down_mm` ([`pass_z_mm`]), with `last_pass`'s thickness `t`:
/// the usual passes to `depth − t`, then one at the full depth. Without a
/// thickness, or with none set, the usual passes. A thickness as deep as
/// the cut leaves the last pass the only one.
pub(super) fn pass_z(
    op: &ProfileOp,
    thickness_mm: f64,
    depth_mm: f64,
    step_down_mm: f64,
    last_pass: Option<&LastPass>,
) -> Result<Vec<f64>, CamError> {
    let full = pass_z_mm(
        &op.id,
        thickness_mm,
        depth_mm,
        step_down_mm,
        op.through_cut_allowance_mm,
    )?;
    let Some(last_mm) = last_pass.and_then(|l| l.thickness_mm) else {
        return Ok(full);
    };
    let rough_mm = depth_mm - last_mm;
    let mut z_mm = if rough_mm > EPS_MM {
        pass_z_mm(
            &op.id,
            thickness_mm,
            rough_mm,
            step_down_mm,
            op.through_cut_allowance_mm,
        )?
    } else {
        Vec::new()
    };
    z_mm.push(thickness_mm - depth_mm);
    Ok(z_mm)
}

/// What [`twins`] needs to know about one outline and its paths.
pub(super) struct Outline<'a> {
    /// The region the outline bounds a side of.
    pub region: &'a Region,
    /// The outline.
    pub contour: &'a Contour,
    /// The shape the outline is.
    pub shape: &'a ShapeRef,
    /// Which way the part's paths were offset from it.
    pub how: Offset,
    /// The part's paths: the outline offset by the tool radius.
    pub centres: &'a [Contour],
}

/// The allowance paths of one outline (#284): the outline offset by the
/// radius plus `allowance_mm` the same way as its part paths, one for
/// each of `outline.centres`, in their order. The design's "band and
/// narrowness checks with `D + 2a`" (A8 step 2):
///
/// - Inside, an allowance path that comes closer than `r + a` to another
///   line of the region would cut that line's skin, or the part beyond
///   it: [`CamError::BandNarrowerThanAllowance`] naming both lines.
/// - Where the outline is wider than the bit but narrower than `D + 2a`
///   (a neck, a slot, a small hole, the mouth of an inlet), the
///   allowance offset vanishes, splits or merges, so its loops are not
///   one for each part path; or a part path runs farther than the radius
///   from every allowance path, into a stretch the earlier passes skip,
///   where the last pass would cut a full slot to the full depth:
///   [`CamError::ShapeNarrowerThanAllowance`].
pub(super) fn twins(
    op: &ProfileOp,
    tool: &Tool,
    outline: &Outline<'_>,
    shapes: &[(ShapeRef, Contour)],
    allowance_mm: f64,
    tolerance_mm: f64,
) -> Result<Vec<Contour>, CamError> {
    let radius_mm = tool.diameter_mm / 2.0;
    let narrow = || CamError::ShapeNarrowerThanAllowance {
        op: op.id.clone(),
        shape: outline.shape.clone(),
        diameter_mm: tool.diameter_mm,
        allowance_mm,
    };
    let reach_mm = radius_mm + allowance_mm;
    let found = match outline.how {
        Offset::Grow => offset(outline.contour, reach_mm),
        Offset::Shrink => offset(outline.contour, -reach_mm),
        // Refused before: On line has no allowance.
        Offset::None => return Err(narrow()),
    };
    if found.len() != outline.centres.len() {
        return Err(narrow());
    }
    check_band(
        &op.id,
        op.side,
        tool,
        allowance_mm,
        outline.region,
        (outline.contour, outline.shape),
        &found,
        shapes,
        tolerance_mm,
    )?;
    // Each part path with the allowance path nearest it, never one twice.
    let mut left: Vec<Option<Contour>> = found.into_iter().map(Some).collect();
    let mut twins = Vec::with_capacity(left.len());
    for centre in outline.centres {
        let nearest = left
            .iter()
            .enumerate()
            .filter_map(|(k, t)| {
                t.as_ref()
                    .map(|t| (k, distance_mm(centre.polyline(), t.polyline())))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k);
        let Some(twin) = nearest.and_then(|k| left.get_mut(k)).and_then(Option::take) else {
            return Err(narrow());
        };
        if farthest_mm(centre.polyline(), twin.polyline(), radius_mm / 4.0) > radius_mm {
            return Err(narrow());
        }
        twins.push(twin);
    }
    Ok(twins)
}

/// #284: an allowance path `twin` of `shape`, an outline of region
/// `region`, must keep the tool radius from every line of every other
/// region, or its passes would cut into that part (S9 holds them to the
/// radius; only a lap at the radius itself is D6's warning instead):
/// [`CamError::BandNarrowerThanAllowance`] naming both shapes, in drawing
/// order, and the gap between them.
#[allow(
    clippy::too_many_arguments,
    reason = "every argument is one fact about the gap; a struct would only rename them"
)]
pub(super) fn check_neighbours(
    op: &ProfileOp,
    tool: &Tool,
    regions: &[Region],
    shapes: &[(ShapeRef, Contour)],
    (twin, shape, contour): (&Polyline, &ShapeRef, &Contour),
    region: usize,
    allowance_mm: f64,
) -> Result<(), CamError> {
    let radius_mm = tool.diameter_mm / 2.0;
    let drawn = |s: &ShapeRef| shapes.iter().position(|(x, _)| x == s);
    for (index, other_region) in regions.iter().enumerate() {
        if index == region {
            continue;
        }
        for (line, other) in region_shapes(other_region, shapes) {
            if !within(twin, line.polyline(), radius_mm) {
                continue;
            }
            let (first, second) = if drawn(shape) <= drawn(other) {
                (shape, other)
            } else {
                (other, shape)
            };
            return Err(CamError::BandNarrowerThanAllowance {
                op: op.id.clone(),
                shape: first.clone(),
                other: Box::new(second.clone()),
                band_mm: distance_mm(contour.polyline(), line.polyline()),
                diameter_mm: tool.diameter_mm,
                allowance_mm,
            });
        }
    }
    Ok(())
}

/// How far the point of `path` farthest from `other` lies from it, mm,
/// sampled at every vertex and at most `step_mm` apart along each
/// segment.
fn farthest_mm(path: &Polyline, other: &Polyline, step_mm: f64) -> f64 {
    let gap = |p: Point| nearest_point(other, p).map_or(f64::INFINITY, |n| n.gap_mm);
    let mut farthest: f64 = 0.0;
    for segment in path.segments() {
        let length_mm = segment.length_mm();
        let mut along_mm = 0.0;
        loop {
            if let Some((p, _)) = segment.point_at(along_mm) {
                farthest = farthest.max(gap(p));
            }
            if along_mm >= length_mm {
                break;
            }
            along_mm = (along_mm + step_mm.max(EPS_MM)).min(length_mm);
        }
    }
    farthest
}

/// `path`, closed, run the other way from the same start: its last pass
/// reversed (#284).
pub(super) fn reversed_from_start(path: &Polyline) -> Polyline {
    let reversed = path.reversed();
    let mut vertices: Vec<Vertex> = reversed.vertices().to_vec();
    // `reversed` starts at the last vertex; the first is now its last.
    if let Some(first) = vertices.pop() {
        vertices.insert(0, first);
    }
    Polyline::new(vertices, true)
}

/// `layout`, the tabs of the closed `path`, carried onto `onto`, which
/// runs beside it (`reversed`: the other way round), each span widened by
/// `widen_mm` at both ends ([`PathTabs::carried`], design A8 step 3): a
/// distance along `path` lands where `onto` comes nearest its point.
pub(super) fn carry(
    path: &Polyline,
    layout: &PathTabs,
    onto: &Polyline,
    reversed: bool,
    widen_mm: f64,
) -> PathTabs {
    let along = |distance_mm: f64| {
        path.point_at(distance_mm)
            .and_then(|(p, _)| nearest_point(onto, p))
            .map_or(0.0, |n| n.along_mm)
    };
    layout.carried(onto.length_mm(), along, reversed, widen_mm)
}
