// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The check that the tool paths of two shapes in one Profile do not cut
//! into each other (`docs/research/toolpath-ordering.md`, decision D6).
//! DreamCarve cuts each shape on its own rather than merging close outlines
//! as PureCutCNC's `edgeLevelRuns` does, so it warns instead.
//!
//! The tool sweeps a band `r` wide either side of its centre path. That band
//! reaches the material another shape keeps exactly when the path comes
//! closer than `r` to that shape's line: next to the line, its kept side is
//! the material of the region it bounds (the part, or for a hole the frame
//! around it). Under Inside every path stays inside its own region, at
//! least `r` from every line of it (`generate_profile` refuses a band
//! narrower than the tool, #192), so at least `r` from every other
//! region's line too, and only Outside and On line cuts can warn. A part
//! and its own holes are one region and are never compared here: Outside,
//! cutting close to them leaves a thin wall, not a gouge; On line, lines
//! of one region closer than the tool's diameter would cut through the
//! part between them, which `generate_profile` refuses (#199).
//!
//! An open path an On line Profile cuts (#175) is its own line and its own
//! tool-centre path, compared by the same On line rule (#206). It belongs
//! to the region whose material holds all of it, as a hole does, and is
//! not compared with that region's lines: there it cuts into the part, not
//! beside it, and `generate_profile`'s band check refuses one closer than
//! the diameter to them away from where they meet (#210). An open path held by no region (in the waste, in a hole's
//! slug, or crossing a line) is a group of its own and is compared with
//! every other shape, and with itself (#224): where two of its stretches
//! far apart along it come closer than the radius, as in a tight spiral or
//! a hairpin, it warns as two separate paths there would. Engrave takes no part: it cuts every line as drawn,
//! so lines that cross or meet are the design, not shapes too close. Nor
//! does a Left or Right Profile (#207): a cut of one that comes within the
//! radius of another line is `generate_profile`'s error, since the line's
//! other side is the part.

use crate::error::ShapeRef;
use crate::profile::MEETING_REACH_DIAMETERS;
use crate::warning::Warning;
use dc_geometry::{Contour, EPS_MM, Polyline, Rect, Region, SelfBand, distance_mm};
use dc_project::{Side, Tool};

/// One tool-centre path in cut order, with the shape it cuts.
pub(crate) type CutPath<'a> = (&'a Polyline, Option<&'a ShapeRef>);

/// One [`Warning::ToolpathsOverlap`] per pair of shapes, from different
/// regions, where either one's tool path comes closer than the tool radius
/// to the other's line. `paths` are in cut order; so are the warnings, by
/// the first path of each pair and then the second, and the shape cut
/// first is named first. `shapes` are the resolved closed targets,
/// `regions` what `nest` made of them, and `open` the open paths cut On
/// line, as drawn (#206).
///
/// An open path no region holds is also measured against itself (#224,
/// [`SelfBand`], with the project's flatten tolerance `tolerance_mm` for
/// where it meets itself): where two of its stretches more than
/// [`MEETING_REACH_DIAMETERS`]` · d` apart along it come closer than the
/// radius, it warns with itself as both shapes, before its pairs with
/// later paths. Its sharp corners and the points where it touches or
/// crosses itself are set aside as `generate_profile`'s band check sets
/// them aside (#221).
///
/// `paths` include a Profile's stock paths (#284), which run
/// `stock_to_leave_mm` farther from their lines than the radius, so the gap
/// the warning says two shapes need grows by it.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "every argument is one fact about the cut; a struct would only rename them"
)]
pub(crate) fn overlap_warnings(
    paths: &[CutPath<'_>],
    shapes: &[(ShapeRef, Contour)],
    open: &[(ShapeRef, Polyline)],
    regions: &[Region],
    side: Side,
    tool: &Tool,
    stock_to_leave_mm: f64,
    tolerance_mm: f64,
) -> Vec<Warning> {
    let radius_mm = tool.diameter_mm / 2.0;
    // Each path that has a known shape, with that shape's line and the
    // group it is never compared within: a closed shape's region; for an
    // open path the region whose material holds it, or else a group of
    // its own past the regions.
    let placed: Vec<(&Polyline, &ShapeRef, &Polyline, usize)> = paths
        .iter()
        .filter_map(|(path, shape)| {
            let shape = (*shape)?;
            if let Some((_, contour)) = shapes.iter().find(|(s, _)| s == shape) {
                let region = regions.iter().position(|r| {
                    std::iter::once(r.outer())
                        .chain(r.holes())
                        .any(|c| c == contour || *c == contour.reversed())
                })?;
                return Some((*path, shape, contour.polyline(), region));
            }
            let (index, (_, line)) = open.iter().enumerate().find(|(_, (s, _))| s == shape)?;
            let group = regions
                .iter()
                .position(|r| region_holds_path(r, line))
                .unwrap_or(regions.len() + index);
            Some((*path, shape, line, group))
        })
        .collect();

    let warning = |a: &ShapeRef, b: &ShapeRef, gap_mm: f64| Warning::ToolpathsOverlap {
        shape: a.clone(),
        other: b.clone(),
        gap_mm,
        tool: tool.name.clone(),
        tool_id: tool.id.clone(),
        diameter_mm: tool.diameter_mm,
        needed_mm: needed_gap_mm(side, tool.diameter_mm) + stock_to_leave_mm,
    };
    let mut warned: Vec<(&ShapeRef, &ShapeRef)> = Vec::new();
    let mut warnings = Vec::new();
    for (index, &(path_a, shape_a, line_a, region_a)) in placed.iter().enumerate() {
        // #224: an open path in the waste against itself, as D6 compares
        // two paths there: at the radius.
        if region_a >= regions.len() && !line_a.is_closed() && !warned.contains(&(shape_a, shape_a))
        {
            let band = SelfBand::new(
                line_a,
                MEETING_REACH_DIAMETERS * tool.diameter_mm,
                tolerance_mm.max(EPS_MM),
            );
            if let Some(gap_mm) = band.narrowest_mm(radius_mm - EPS_MM, |_, _| true) {
                warned.push((shape_a, shape_a));
                warnings.push(warning(shape_a, shape_a, gap_mm));
            }
        }
        for &(path_b, shape_b, line_b, region_b) in placed.iter().skip(index + 1) {
            if region_a == region_b
                || warned
                    .iter()
                    .any(|&(x, y)| (x, y) == (shape_a, shape_b) || (x, y) == (shape_b, shape_a))
            {
                continue;
            }
            if !(within(path_a, line_b, radius_mm) || within(path_b, line_a, radius_mm)) {
                continue;
            }
            warned.push((shape_a, shape_b));
            warnings.push(warning(shape_a, shape_b, distance_mm(line_a, line_b)));
        }
    }
    warnings
}

/// True when `path` comes closer than `radius_mm` to `line`, by more than
/// [`EPS_MM`]: shapes exactly a tool's reach apart are fine.
pub(crate) fn within(path: &Polyline, line: &Polyline, radius_mm: f64) -> bool {
    let reach_mm = radius_mm - EPS_MM;
    let near = match (path.bounds(), line.bounds()) {
        (Some(a), Some(b)) => boxes_within(a, b, reach_mm),
        _ => false,
    };
    near && distance_mm(path, line) < reach_mm
}

/// True when every point of the open path `path` lies in `region`'s
/// material or on its lines (#206): each vertex and the middle of each
/// segment, so a line or arc that runs out of the region between two
/// vertices inside it does not count.
pub(crate) fn region_holds_path(region: &Region, path: &Polyline) -> bool {
    let middles = path
        .segments()
        .filter_map(|s| s.point_at(s.length_mm() / 2.0).map(|(p, _)| p));
    path.vertices()
        .iter()
        .map(|v| v.point())
        .chain(middles)
        .all(|p| region.contains_point(p))
}

/// True when boxes `a` and `b` are less than `reach_mm` apart on each axis.
fn boxes_within(a: Rect, b: Rect, reach_mm: f64) -> bool {
    a.min.x - b.max.x < reach_mm
        && b.min.x - a.max.x < reach_mm
        && a.min.y - b.max.y < reach_mm
        && b.min.y - a.max.y < reach_mm
}

/// The gap two shapes need for this side not to cut into each other: both
/// tools run between them on Outside, half of each on the line.
fn needed_gap_mm(side: Side, diameter_mm: f64) -> f64 {
    match side {
        Side::Outside | Side::Inside | Side::Left | Side::Right => diameter_mm,
        Side::OnLine => diameter_mm / 2.0,
    }
}

#[cfg(test)]
mod tests;
