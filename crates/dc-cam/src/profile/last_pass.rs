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
//! distance, for a closed outline and, Left or Right, for an open path
//! (#311); this module makes it, refuses a drawing it cannot follow,
//! and carries the part path's tabs onto it.

use super::{Offset, check_band, region_shapes};
use crate::error::{CamError, ShapeRef};
use crate::overlap::within;
use crate::passes::pass_z_mm;
use crate::tabs::{PathTabs, RaisedSpan};
use dc_geometry::{
    Contour, EPS_MM, Point, Polyline, Region, Vec2, Vertex, distance_mm, nearest_point, offset,
    offset_open,
};
use dc_project::{LastPass, ProfileOp, Side, TabShape, Tool};
use std::collections::BTreeMap;

/// Refuses a last pass the generator cannot cut (validation reports each
/// first): one On line, which has no wall to leave material on
/// ([`CamError::LastPassNeedsSide`]; Left and Right have one since #311),
/// an allowance that is not more than
/// 0 and at most a third of the tool's `diameter_mm`, or a thickness that
/// is not more than 0 and at most `depth_mm` and the `step_down_mm` in
/// effect (#310 review: the last pass is never thicker than a step)
/// ([`CamError::InvalidLastPass`]).
pub(super) fn check(
    op: &ProfileOp,
    last_pass: &LastPass,
    diameter_mm: f64,
    depth_mm: f64,
    step_down_mm: f64,
) -> Result<(), CamError> {
    if !op.side.has_wall() {
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
    if let Some(thickness_mm) = last_pass.thickness_mm
        && thickness_mm > step_down_mm + EPS_MM
    {
        return Err(CamError::InvalidLastPass {
            op: op.id.clone(),
            field: "thickness_mm",
            value: thickness_mm,
            range: format!("at most the {step_down_mm} mm step-down"),
        });
    }
    Ok(())
}

/// The pass Zs of a cut `depth_mm` deep into stock `thickness_mm` thick
/// at `step_down_mm` ([`pass_z_mm`]), with `last_pass`'s thickness `t`:
/// the usual passes to `depth − t`, then one at the full depth. Without a
/// thickness, or with none set, the usual passes.
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

/// How much more than `√2 · allowance` the last pass may take off
/// without being looked at further, mm (#310 review): the rounding of
/// offsets and sampling, so that a square corner, where it takes exactly
/// that, is cut. Also how much farther than the allowance a stretch must
/// run from the allowance path to be one it stops short of (#324).
pub(super) const THICKNESS_SLACK_MM: f64 = 0.05;

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
///   one for each part path: [`CamError::ShapeNarrowerThanAllowance`].
/// - The last pass takes off, at any point, at most how far its path
///   runs from the allowance path there ([`cuts_only_corners`]; #310
///   review, changed in #324): nowhere more than a third of the bit (or
///   `√2 · a` plus [`THICKNESS_SLACK_MM`], a square corner's, where that
///   is more), and more than a square corner's only at a corner, where
///   the earlier passes cut both walls and stop short of the wedge
///   between. A notch, slot or inlet the earlier passes bridge and skip,
///   or a tip so narrow that its wedge is thicker, would leave the last
///   pass that much to cut at full depth:
///   [`CamError::ShapeNarrowerThanAllowance`] too.
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
    paired(
        outline.centres.iter().map(Contour::polyline),
        found,
        Contour::polyline,
        allowance_mm,
        tool.diameter_mm,
    )
    .ok_or_else(narrow)
}

/// The allowance paths of one open path `path` cut Left or Right (#311):
/// `path` offset by the radius plus `allowance_mm` to the side of
/// `left_mm` (positive to the left, the last pass's offset) with
/// [`offset_open`], one for each of the last pass's pieces `centres`, in
/// their order. Each ends square on the normals through the path's end
/// vertices, as the last pass's do, so the earlier passes stop where the
/// last pass stops: neither cuts past an end, and the last pass takes the
/// allowance off right up to it. The narrowness rule is [`twins`']: where
/// `path` fits the bit but not the bit plus the allowance (an arc, a U or
/// a hairpin on the cut side), the offset loses or gains pieces; and the
/// last pass takes off what [`cuts_only_corners`] allows, a departure
/// that reaches an end of the path counting as no corner. Either is
/// [`CamError::ShapeNarrowerThanAllowance`].
pub(super) fn open_twins(
    op: &ProfileOp,
    tool: &Tool,
    (shape, path): (&ShapeRef, &Polyline),
    centres: &[Polyline],
    left_mm: f64,
    allowance_mm: f64,
) -> Result<Vec<Polyline>, CamError> {
    let reach_mm = tool.diameter_mm / 2.0 + allowance_mm;
    fn itself(p: &Polyline) -> &Polyline {
        p
    }
    let found = offset_open(path, reach_mm.copysign(left_mm));
    paired(
        centres.iter(),
        found,
        itself,
        allowance_mm,
        tool.diameter_mm,
    )
    .ok_or_else(|| CamError::ShapeNarrowerThanAllowance {
        op: op.id.clone(),
        shape: shape.clone(),
        diameter_mm: tool.diameter_mm,
        allowance_mm,
    })
}

/// Each of the last pass's paths `centres` with the allowance path of
/// `found` nearest it, never one twice, in `centres`' order; `None` when
/// the two are not one for one, or the last pass on a path would take
/// off more than [`cuts_only_corners`] allows after the allowance path
/// paired with it. `line` gives an allowance path's polyline.
fn paired<'c, T>(
    centres: impl ExactSizeIterator<Item = &'c Polyline>,
    found: Vec<T>,
    line: impl Fn(&T) -> &Polyline,
    allowance_mm: f64,
    diameter_mm: f64,
) -> Option<Vec<T>> {
    if found.len() != centres.len() {
        return None;
    }
    let mut left: Vec<Option<T>> = found.into_iter().map(Some).collect();
    let mut twins = Vec::with_capacity(left.len());
    for centre in centres {
        let nearest = left
            .iter()
            .enumerate()
            .filter_map(|(k, t)| t.as_ref().map(|t| (k, distance_mm(centre, line(t)))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k);
        let twin = nearest
            .and_then(|k| left.get_mut(k))
            .and_then(Option::take)?;
        // The last pass takes off at most how far its path runs from the
        // allowance path's (its cutter reaches no farther past theirs).
        let step_mm = (diameter_mm / 16.0).min(0.25);
        if !cuts_only_corners(centre, line(&twin), allowance_mm, diameter_mm, step_mm) {
            return None;
        }
        twins.push(twin);
    }
    Some(twins)
}

/// #284: an allowance path `twin` of `shape`, an outline of region
/// `region`, must keep the tool radius from every line of every other
/// region and, Outside, lie outside every other region's material, or its
/// passes would cut into that part (S9 holds them to the radius; only a
/// lap at the radius itself is D6's warning instead):
/// [`CamError::BandNarrowerThanAllowance`] naming both shapes, in drawing
/// order, and the gap between them. Inside, an allowance path lies in its
/// own region, which no other region's material overlaps.
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
        // #310 review: a path one radius or more past another part's line
        // is inside that part, which `within` does not see.
        let inside = op.side == Side::Outside
            && twin
                .vertices()
                .first()
                .is_some_and(|v| other_region.contains_point(v.point()));
        for (line, other) in region_shapes(other_region, shapes) {
            if !inside && !within(twin, line.polyline(), radius_mm) {
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
                needed_mm: tool.diameter_mm + allowance_mm,
            });
        }
    }
    Ok(())
}

/// The most the last pass may take off anywhere, mm (#324): a third of
/// the tool's `diameter_mm`, the thickest skin [`check`] lets the
/// allowance itself be, so a corner's wedge is never thicker than a
/// straight wall's skin may be; or a square corner's `√2 · a`, plus
/// [`THICKNESS_SLACK_MM`], where that is more (the #310 rule, so nothing
/// it cut is refused now).
fn thickest_mm(diameter_mm: f64, allowance_mm: f64) -> f64 {
    (diameter_mm / 3.0).max(square_corner_mm(allowance_mm))
}

/// What the last pass takes off in a square corner, `√2 · a`, plus
/// [`THICKNESS_SLACK_MM`], mm: the #310 bound, within which no stretch is
/// looked at further.
fn square_corner_mm(allowance_mm: f64) -> f64 {
    std::f64::consts::SQRT_2 * allowance_mm + THICKNESS_SLACK_MM
}

/// How far a corner may turn back the other way, rad (#324): 1°, for the
/// sampling and flattening of curves. A stretch that turns back more goes
/// in and comes out again.
const CORNER_TURN_BACK_RAD: f64 = std::f64::consts::PI / 180.0;

/// How far a corner may turn, rad (#324): 179°, a U-turn less the
/// [`CORNER_TURN_BACK_RAD`] allowed for sampling. A stretch that turns as
/// far as a U-turn goes in and comes out again: a slot's end.
const CORNER_TURN_RAD: f64 = std::f64::consts::PI - CORNER_TURN_BACK_RAD;

/// One stretch of a part path the allowance path stops short of, for
/// [`cuts_only_corners`].
struct Departure {
    /// The farthest it lies from the allowance path, mm.
    thickest_mm: f64,
    /// How far it turns left and right, rad.
    left_rad: f64,
    right_rad: f64,
    /// Its direction of travel at the last sample.
    heading: Vec2,
}

impl Departure {
    /// A corner turns one way, less than a U-turn.
    fn is_corner(&self) -> bool {
        self.left_rad.min(self.right_rad) <= CORNER_TURN_BACK_RAD
            && self.left_rad.max(self.right_rad) < CORNER_TURN_RAD
    }
}

/// Whether the last pass on `path` may follow passes on `twin`, its
/// allowance path at `allowance_mm`, with a tool of `diameter_mm` (#324),
/// `path` sampled at every vertex and at most `step_mm` apart along each
/// segment. The last pass takes off at most how far its path runs from
/// the allowance path, and:
///
/// - nowhere more than [`thickest_mm`];
/// - where it takes off more than a square corner's `√2 · a` (plus
///   [`THICKNESS_SLACK_MM`]), only at a corner: each stretch of `path`
///   farther than `a` (plus the slack) from `twin` that reaches past that
///   must turn one way, by less than a U-turn ([`Departure::is_corner`]).
///   A corner's earlier passes cut both its walls and stop short of where
///   they meet, so the last pass takes only the wedge between. A stretch
///   that turns back, or turns as far as a U-turn, goes into a notch,
///   slot or inlet that the earlier passes bridge and skip, and the last
///   pass would cut it whole, as wide as the bit, at full depth. On an
///   open path (#311) a stretch that reaches either end is no corner.
fn cuts_only_corners(
    path: &Polyline,
    twin: &Polyline,
    allowance_mm: f64,
    diameter_mm: f64,
    step_mm: f64,
) -> bool {
    let gap = |p: Point| nearest_point(twin, p).map_or(f64::INFINITY, |n| n.gap_mm);
    let mut samples: Vec<(f64, Vec2)> = Vec::new();
    for segment in path.segments() {
        let length_mm = segment.length_mm();
        let mut along_mm = 0.0;
        loop {
            if let Some((p, heading)) = segment.point_at(along_mm) {
                samples.push((gap(p), heading));
            }
            if along_mm >= length_mm {
                break;
            }
            along_mm = (along_mm + step_mm.max(EPS_MM)).min(length_mm);
        }
    }
    let square_mm = square_corner_mm(allowance_mm);
    if samples
        .iter()
        .any(|(gap_mm, _)| *gap_mm > thickest_mm(diameter_mm, allowance_mm))
    {
        return false;
    }
    if samples.iter().all(|(gap_mm, _)| *gap_mm <= square_mm) {
        return true;
    }
    let away = |gap_mm: f64| gap_mm > allowance_mm + THICKNESS_SLACK_MM;
    let n = samples.len();
    // Start at a sample beside the allowance path, so no stretch wraps the
    // scan; a path nowhere beside it is one long stretch, no corner. An
    // open path (#311) is scanned from its start to its end, and a
    // stretch that reaches either end is no corner: the allowance path
    // stops short of the end, which the last pass would cut whole.
    let closed = path.is_closed();
    let order: Vec<usize> = if closed {
        let Some(beside) = samples.iter().position(|(gap_mm, _)| !away(*gap_mm)) else {
            return false;
        };
        (1..=n).map(|offset| (beside + offset) % n).collect()
    } else {
        (0..n).collect()
    };
    if !closed {
        // The stretches at the ends: no corners, so no thicker than a
        // square corner's.
        let mut at_ends = samples
            .iter()
            .take_while(|(gap_mm, _)| away(*gap_mm))
            .chain(samples.iter().rev().take_while(|(gap_mm, _)| away(*gap_mm)));
        if at_ends.any(|(gap_mm, _)| *gap_mm > square_mm) {
            return false;
        }
    }
    let mut stretch: Option<Departure> = None;
    for k in order {
        let Some(&(gap_mm, heading)) = samples.get(k) else {
            continue;
        };
        if away(gap_mm) {
            let d = stretch.get_or_insert(Departure {
                thickest_mm: 0.0,
                left_rad: 0.0,
                right_rad: 0.0,
                heading,
            });
            let turn_rad = d.heading.cross(heading).atan2(d.heading.dot(heading));
            if turn_rad > 0.0 {
                d.left_rad += turn_rad;
            } else {
                d.right_rad -= turn_rad;
            }
            d.heading = heading;
            d.thickest_mm = d.thickest_mm.max(gap_mm);
        } else if let Some(d) = stretch.take()
            && d.thickest_mm > square_mm
            && !d.is_corner()
        {
            return false;
        }
    }
    true
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

/// `layout`, the tabs of the closed `path`, carried onto `back`, the same
/// path run the other way ([`reversed_from_start`]), so they stay where
/// they are ([`PathTabs::carried`]): a distance along `path` lands where
/// `back` passes its point.
pub(super) fn carry_reversed(path: &Polyline, layout: &PathTabs, back: &Polyline) -> PathTabs {
    let along = |distance_mm: f64| {
        path.point_at(distance_mm)
            .and_then(|(p, _)| nearest_point(back, p))
            .map_or(0.0, |n| n.along_mm)
    };
    layout.carried(back.length_mm(), along, true, 0.0)
}

/// One sample of the last pass for [`twin_tabs`]: its cutter centre,
/// its Z, and the top and shape of the tab it rises over there.
type LastCut = (Point, f64, Option<(f64, TabShape)>);

/// One sample of an allowance path for [`twin_tabs`]: its distance
/// along the path and, where a tab is in reach, the height it must keep
/// above, the top of that tab, and whether every tab in reach is smooth.
type Need = (f64, Option<(f64, f64, bool)>);

/// The sampling step of [`twin_tabs`], mm: a twelfth of the tool radius,
/// at least 0.02 and at most 0.25.
fn tab_step_mm(radius_mm: f64) -> f64 {
    (radius_mm / 12.0).clamp(0.02, 0.25)
}

/// Points of `path` every `step_mm` along it from its start (and its
/// end), with their distance along it, walking only segments longer
/// than [`EPS_MM`], as [`Polyline::point_at`] does.
fn path_samples(path: &Polyline, step_mm: f64) -> Vec<(f64, Point)> {
    let mut out = Vec::new();
    let mut walked_mm = 0.0;
    let mut next_mm = 0.0;
    for segment in path.segments() {
        let length_mm = segment.length_mm();
        if length_mm <= EPS_MM {
            continue;
        }
        while next_mm <= walked_mm + length_mm {
            if let Some((p, _)) = segment.point_at(next_mm - walked_mm) {
                out.push((next_mm, p));
            }
            next_mm += step_mm;
        }
        walked_mm += length_mm;
    }
    out
}

/// Points bucketed by a square cell, for finding the points near one.
struct Buckets<T> {
    cell_mm: f64,
    cells: BTreeMap<(i64, i64), Vec<(Point, T)>>,
}

impl<T: Copy> Buckets<T> {
    fn new(cell_mm: f64, points: impl IntoIterator<Item = (Point, T)>) -> Self {
        let mut buckets = Self {
            cell_mm,
            cells: BTreeMap::new(),
        };
        for (p, value) in points {
            let key = buckets.key(p);
            buckets.cells.entry(key).or_default().push((p, value));
        }
        buckets
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "cell indices of coordinates within MAX_COORDINATE_MM"
    )]
    fn key(&self, p: Point) -> (i64, i64) {
        (
            (p.x / self.cell_mm).floor() as i64,
            (p.y / self.cell_mm).floor() as i64,
        )
    }

    /// The values of the points closer than `reach_mm`, at most the cell
    /// size, to `p`.
    fn near(&self, p: Point, reach_mm: f64) -> impl Iterator<Item = T> + '_ {
        let (i, j) = self.key(p);
        (-1..=1)
            .flat_map(move |di| (-1..=1).map(move |dj| (i + di, j + dj)))
            .filter_map(|key| self.cells.get(&key))
            .flatten()
            .filter(move |(q, _)| q.distance_to(p) < reach_mm)
            .map(|(_, value)| *value)
    }
}

/// The tabs of an allowance path `twin` (#310 review, replacing design A8
/// step 3's widening by `a`), from the tabs the last pass leaves on its
/// path `path` (`layout`, its floor at `floor_z_mm`), cut with a tool of
/// `radius_mm`.
///
/// A tab is the stock the last pass leaves standing above its floor: a
/// point within the radius of the path stands as high as the lowest the
/// last pass cuts within the radius of it. Each point of `twin` is raised
/// to at least the highest such point within the radius of it, so no
/// pass before the last cuts any of it, however the two paths turn: at a
/// sharp corner the allowance path runs past the corner beside the far
/// edge's tabs, which an arc length along it does not see. The heights are
/// sampled every [`tab_step_mm`], the stock on a lattice of that step
/// and, all round each raised cut, just inside its reach (#311: where the
/// last pass rises onto a tab, the tab's foot at the far side of its kerf
/// is a cusp thinner than the lattice, which the allowance pass reaches
/// first); the cutter is taken that much wider and each raised stretch
/// that much longer at both ends, so the lattice and sampling never let a
/// cut through. On an open path (#311) the stretches stop at its ends. A
/// stretch is rect if any tab it carries is, else smooth, its
/// ramps no longer than keeps it at or above every height sampled under
/// them. Each earlier pass at `z` cuts a smooth stretch's fraction `f` of
/// the way from `z` up to its top; the heights are the last pass's, at
/// its floor, which is lower, so the same fraction is at least as high
/// there.
pub(super) fn twin_tabs(
    path: &Polyline,
    layout: &PathTabs,
    twin: &Polyline,
    radius_mm: f64,
    floor_z_mm: f64,
) -> PathTabs {
    let twin_mm = twin.length_mm();
    let closed = twin.is_closed();
    let none = || PathTabs::from_spans(twin_mm, layout.tab_count(), &[], closed);
    if layout.is_untabbed() || twin_mm <= EPS_MM || path.length_mm() <= EPS_MM {
        return none();
    }
    let step_mm = tab_step_mm(radius_mm);
    let raised = |z_mm: f64| z_mm > floor_z_mm + EPS_MM;
    // The last pass: its cutter centre every half step, with its Z and
    // the top and shape of the tab it rises over.
    let last: Vec<LastCut> = path_samples(path, step_mm / 2.0)
        .into_iter()
        .map(|(d, p)| (p, layout.z_at_mm(d, floor_z_mm), layout.span_at(d)))
        .collect();
    let cuts = Buckets::new(radius_mm, last.iter().map(|&(p, z, _)| (p, z)));
    // The tabs: lattice points within the radius of a raised cut, at the
    // height the last pass leaves them, with the top and shape of the
    // tab whose cut stands over them.
    let mut lattice: BTreeMap<(i64, i64), (f64, TabShape)> = BTreeMap::new();
    let reach = lattice_reach(radius_mm, step_mm);
    for &(p, z, span) in &last {
        let Some(span) = span.filter(|_| raised(z)) else {
            continue;
        };
        let (ci, cj) = lattice_index(p, step_mm);
        for di in -reach..=reach {
            for dj in -reach..=reach {
                lattice
                    .entry((ci + di, cj + dj))
                    .and_modify(|(top, shape)| {
                        *top = top.max(span.0);
                        if span.1 == TabShape::Rect {
                            *shape = TabShape::Rect;
                        }
                    })
                    .or_insert(span);
            }
        }
    }
    let mut material: Vec<(Point, (f64, f64, TabShape))> = lattice
        .into_iter()
        .filter_map(|((i, j), (top, shape))| {
            let p = lattice_point(i, j, step_mm);
            let height = cuts.near(p, radius_mm).reduce(f64::min)?;
            raised(height).then_some((p, (height, top, shape)))
        })
        .collect();
    // #311: the tabs' edges too, all round each raised cut just inside
    // its reach. Where the last pass rises onto a tab, the tab's foot at
    // the far side of its kerf is a cusp thinner than the lattice, between
    // the circles of reach of the last cut below and the first above, and
    // the allowance pass `a` nearer that side reaches it first.
    let ring = ring_offsets(radius_mm - RING_INSET_MM, step_mm);
    for &(c, z, span) in &last {
        let Some((top, shape)) = span.filter(|_| raised(z)) else {
            continue;
        };
        for &u in &ring {
            let p = c + u;
            if let Some(height) = cuts.near(p, radius_mm).reduce(f64::min)
                && raised(height)
            {
                material.push((p, (height, top, shape)));
            }
        }
    }
    if material.is_empty() {
        return none();
    }
    let stock = Buckets::new(radius_mm + step_mm, material);
    // Each point of the allowance path: the height it must keep above,
    // the tab top and whether every tab under it is smooth.
    let need: Vec<Need> = path_samples(twin, step_mm / 2.0)
        .into_iter()
        .map(|(d, q)| {
            let under = stock.near(q, radius_mm + step_mm).fold(
                None,
                |acc: Option<(f64, f64, bool)>, (height, top, shape)| {
                    let smooth = shape == TabShape::Smooth;
                    Some(match acc {
                        None => (height, top, smooth),
                        Some((h, t, s)) => (h.max(height), t.max(top), s && smooth),
                    })
                },
            );
            (d, under)
        })
        .collect();
    let spans = raised_runs(&need, twin_mm, step_mm, floor_z_mm, closed);
    PathTabs::from_spans(twin_mm, layout.tab_count(), &spans, closed)
}

/// How far inside a raised cut's reach [`twin_tabs`] looks for a tab's
/// edge, mm: less than a sample's spacing squared over the diameter, so
/// the edge beside the first cut above a rise lies beyond the reach of the
/// last cut below it.
const RING_INSET_MM: f64 = 1e-4;

/// Offsets `radius_mm` long all round, at most `step_mm` apart along the
/// circle, for [`twin_tabs`]' tab edges.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a count of steps round a circle of a tool's radius"
)]
fn ring_offsets(radius_mm: f64, step_mm: f64) -> Vec<Vec2> {
    let n = (std::f64::consts::TAU * radius_mm / step_mm)
        .ceil()
        .max(8.0) as usize;
    (0..n)
        .map(|k| {
            let angle = std::f64::consts::TAU * k as f64 / n as f64;
            Vec2::new(angle.cos(), angle.sin()) * radius_mm
        })
        .collect()
}

/// How many lattice steps of `step_mm` reach past the radius.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a radius over a step of at least 0.02 mm"
)]
fn lattice_reach(radius_mm: f64, step_mm: f64) -> i64 {
    (radius_mm / step_mm).ceil() as i64 + 1
}

/// The lattice point nearest `p`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "lattice indices of coordinates within MAX_COORDINATE_MM"
)]
fn lattice_index(p: Point, step_mm: f64) -> (i64, i64) {
    (
        (p.x / step_mm).round() as i64,
        (p.y / step_mm).round() as i64,
    )
}

/// The point of lattice index `(i, j)`.
#[allow(
    clippy::cast_precision_loss,
    reason = "lattice indices of coordinates within MAX_COORDINATE_MM"
)]
fn lattice_point(i: i64, j: i64, step_mm: f64) -> Point {
    Point::new(i as f64 * step_mm, j as f64 * step_mm)
}

/// The raised stretches of an allowance path `length_mm` long, from the
/// heights `need` sampled along it every half `step_mm` ([`twin_tabs`]):
/// each run of samples that must keep above `floor_z_mm`, a step longer
/// at both ends, at the highest top under it, rect unless every tab under
/// it is smooth. On a `closed` path a run may go through its start; on
/// an open one (#311) runs stop at its ends.
fn raised_runs(
    need: &[Need],
    length_mm: f64,
    step_mm: f64,
    floor_z_mm: f64,
    closed: bool,
) -> Vec<RaisedSpan> {
    let up = |k: usize| {
        need.get(k)
            .and_then(|(_, n)| *n)
            .filter(|(h, _, _)| *h > floor_z_mm + EPS_MM)
    };
    let n = need.len();
    // Start at a sample that stands clear, so no run wraps the scan; an
    // open path's scan runs from its start, and ends any run at its end.
    let first_clear = if closed {
        (0..n).find(|&k| up(k).is_none())
    } else {
        (0..n).any(|k| up(k).is_none()).then_some(n - 1)
    };
    let Some(first_clear) = first_clear else {
        let top = need
            .iter()
            .filter_map(|(_, n)| n.map(|(_, t, _)| t))
            .fold(floor_z_mm, f64::max);
        let smooth = need.iter().all(|(_, n)| n.is_none_or(|(_, _, s)| s));
        return vec![RaisedSpan {
            start_mm: 0.0,
            length_mm,
            top_z_mm: top,
            shape: if smooth {
                TabShape::Smooth
            } else {
                TabShape::Rect
            },
            ramp_in_mm: 0.0,
            ramp_out_mm: 0.0,
        }];
    };
    let mut spans = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for offset in 1..=n {
        let k = (first_clear + offset) % n;
        if up(k).is_some() {
            run.push(k);
            if closed || k + 1 < n {
                continue;
            }
        }
        if !run.is_empty() {
            spans.push(raised_span(
                need, &run, length_mm, step_mm, floor_z_mm, closed,
            ));
            run.clear();
        }
    }
    spans
}

/// The raised stretch over the samples `run` of `need`, in order along
/// the path (possibly through its start, when `closed`), for
/// [`raised_runs`]. An open path's stretch stops at its ends.
fn raised_span(
    need: &[Need],
    run: &[usize],
    length_mm: f64,
    step_mm: f64,
    floor_z_mm: f64,
    closed: bool,
) -> RaisedSpan {
    let at = |k: usize| need.get(k).map_or(0.0, |(d, _)| *d);
    let first = run.first().map_or(0.0, |&k| at(k));
    let last = run.last().map_or(first, |&k| at(k));
    let (start_mm, run_mm) = if closed {
        (
            first - step_mm,
            (last - first).rem_euclid(length_mm) + 2.0 * step_mm,
        )
    } else {
        let start_mm = (first - step_mm).max(0.0);
        (start_mm, (last + step_mm).min(length_mm) - start_mm)
    };
    let samples: Vec<(f64, f64)> = run
        .iter()
        .filter_map(|&k| need.get(k).and_then(|(_, n)| n.map(|(h, _, _)| (at(k), h))))
        .collect();
    let top = run
        .iter()
        .filter_map(|&k| need.get(k).and_then(|(_, n)| n.map(|(_, t, _)| t)))
        .fold(floor_z_mm, f64::max);
    let smooth = run
        .iter()
        .all(|&k| need.get(k).is_none_or(|(_, n)| n.is_none_or(|(_, _, s)| s)));
    if !smooth || top <= floor_z_mm + EPS_MM {
        return RaisedSpan {
            start_mm,
            length_mm: run_mm,
            top_z_mm: top,
            shape: TabShape::Rect,
            ramp_in_mm: 0.0,
            ramp_out_mm: 0.0,
        };
    }
    // The longest ramps that keep the stretch at or above every sample:
    // the fraction of the way up from the floor it must be at each.
    let mut ramp_in_mm = run_mm / 2.0;
    let mut ramp_out_mm = run_mm / 2.0;
    for (d, height) in samples {
        let fraction = ((height - floor_z_mm) / (top - floor_z_mm)).clamp(0.0, 1.0);
        if fraction <= 0.0 {
            continue;
        }
        let from_start_mm = (d - start_mm).rem_euclid(length_mm);
        let to_end_mm = run_mm - from_start_mm;
        ramp_in_mm = ramp_in_mm.min(from_start_mm / fraction);
        ramp_out_mm = ramp_out_mm.min(to_end_mm / fraction);
    }
    RaisedSpan {
        start_mm,
        length_mm: run_mm,
        top_z_mm: top,
        shape: TabShape::Smooth,
        ramp_in_mm: ramp_in_mm.max(EPS_MM),
        ramp_out_mm: ramp_out_mm.max(EPS_MM),
    }
}
