// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/xyLead.ts

//! Lead-in and lead-out arcs on a Profile (`docs/specs/phase-2/design.md`,
//! "Algorithms → Lead-in and lead-out"; functional spec capability 17). A
//! plunge or a stop on a wall that stays puts the flute against it at no
//! feed and leaves a mark; a lead is one circular arc, tangent to the
//! tool-centre contour where it joins, so the tool takes up the cut while
//! moving and leaves it the same way.
//!
//! - [`plan_lead_in`] ranks every edge of the contour at fractions
//!   [`LEAD_ARRIVAL_FRACTIONS`] (upstream `contourArrivals`): least corner
//!   turn within one diameter, then least travel from where the tool
//!   comes from, then edge and fraction. For the user's radius and then
//!   [`LEAD_RADIUS_FACTORS`] of it, the user's sweep and then
//!   [`LEAD_SWEEP_FALLBACKS_DEG`] below it, each arrival and each free side
//!   (a probe either side of the contour), the first arc that stays in the
//!   waste domain wins ([`lead_fits`], upstream `validateLead`). The
//!   contour is re-seamed at the arrival ([`reseam`]).
//! - [`plan_lead_out`] continues along the re-seamed contour by the
//!   overcut, then the mirror arc, on the same ladder.
//!
//! Departures from upstream: the radius ladder starts at the user's radius
//! (VCarve's "Leads" radius) instead of one tool diameter, and the sweep
//! ladder at the user's sweep; there is no length budget, since the user
//! sets the size; the overcut is the user's (VCarve's "Overcut
//! Distance"), where upstream searches overlaps of 0 to 1 diameter for a
//! corner-seamed exit. Leads are true arcs, cut as `ArcCw`/`ArcCcw` moves
//! at the cutting feed, not tessellated at a plunge-scaled feed: the
//! descent is finished before the lead starts. Contours carry arcs, so
//! the corner score adds the turn of each arc within the window to the
//! turn at each vertex; on a polygon that is upstream's score.
//!
//! Y-up (ADR-0001): the free side is found by probing, and a lead on the
//! left of travel is counter-clockwise about its centre, on the right
//! clockwise; nothing is copied from upstream's Y-down signs.

use crate::entry::{EntryDomain, boundary_safety_mm};
use crate::error::{CamError, ShapeRef};
use crate::profile::shape_warning;
use crate::toolpath::MoveKind;
use crate::warning::{Warning, WarningCode};
use dc_geometry::{EPS_MM, Point, Polyline, Segment, Vec2, Vertex};
use dc_project::{Lead, OperationId};
use std::f64::consts::PI;

/// Radii tried, as fractions of the user's radius, widest first (upstream
/// `LEAD_RADIUS_FACTORS`, there of the tool diameter).
pub(crate) const LEAD_RADIUS_FACTORS: [f64; 4] = [1.0, 0.75, 0.5, 0.25];
/// Sweeps tried after the user's, degrees, each only when below it
/// (upstream `LEAD_SWEEP_DEGREES` after its 90°).
pub(crate) const LEAD_SWEEP_FALLBACKS_DEG: [f64; 2] = [60.0, 45.0];
/// Fractions along each edge where a lead may join, in the order they are
/// generated (upstream `LEAD_ARRIVAL_FRACTIONS`); the ranking decides.
pub(crate) const LEAD_ARRIVAL_FRACTIONS: [f64; 4] = [0.0, 0.5, 0.25, 0.75];
/// How far either way along the contour a corner counts against an
/// arrival, tool diameters (upstream `LEAD_CORNER_WINDOW_FACTOR`).
pub(crate) const LEAD_CORNER_WINDOW_DIAMETERS: f64 = 1.0;
/// The free-side probe either side of the contour, as a fraction of the
/// tool diameter (upstream `LEAD_SIDE_PROBE_FRACTION`).
pub(crate) const LEAD_SIDE_PROBE_FRACTION: f64 = 0.05;
/// Upstream's arc tessellation step, radians (`DEFAULT_FLATTEN_ARC_STEP`):
/// the corner score's quantum, and the base of the sampling budget.
pub(crate) const LEAD_ARC_STEP_RAD: f64 = PI / 36.0;
/// A lead's far end must be more than this fraction of its radius from
/// the contour point it joins (upstream `MIN_LEAD_OFFSET_FRACTION`).
pub(crate) const MIN_LEAD_OFFSET_FRACTION: f64 = 0.5;

/// Where a lead's tool centre may be.
pub(crate) trait LeadSpace {
    /// True when the tool centre may be at `p`: in the domain or on its
    /// edge. Used within `sqrt(2 · R · safety)` of the join, where the
    /// tool is on the wall it joins.
    fn inside(&self, p: Point) -> bool;
    /// True when `p` is in the domain and at least [`LeadSpace::safety_mm`]
    /// from every wall of it. Used farther from the join, so the arc does
    /// not ride a second wall.
    fn clear_of_walls(&self, p: Point) -> bool;
    /// The boundary safety, mm.
    fn safety_mm(&self) -> f64;
}

/// A Profile path's waste domain as a [`LeadSpace`].
pub(crate) struct DomainSpace<'a> {
    domain: &'a EntryDomain,
    safety_mm: f64,
    slack_mm: f64,
}

impl<'a> DomainSpace<'a> {
    /// `domain` for a tool `tool_diameter_mm` across, its arcs flattened
    /// within `tolerance_mm`: "inside" admits points that far outside the
    /// chords, where a curved wall's tool path runs. The safety is the
    /// entry's ([`boundary_safety_mm`]), as upstream's.
    pub(crate) fn new(domain: &'a EntryDomain, tool_diameter_mm: f64, tolerance_mm: f64) -> Self {
        Self {
            domain,
            safety_mm: boundary_safety_mm(tool_diameter_mm),
            slack_mm: tolerance_mm.max(0.0) + ON_BOUNDARY_MM,
        }
    }
}

/// How far outside the domain a point still counts as on its edge, mm.
const ON_BOUNDARY_MM: f64 = 1e-7;
/// Lengths and angles below this are zero.
const LEAD_EPS: f64 = 1e-9;

impl LeadSpace for DomainSpace<'_> {
    fn inside(&self, p: Point) -> bool {
        self.domain.clearance_mm(p) >= -self.slack_mm
    }

    fn clear_of_walls(&self, p: Point) -> bool {
        self.domain.clearance_mm(p) >= self.safety_mm
    }

    fn safety_mm(&self) -> f64 {
        self.safety_mm
    }
}

/// One lead arc in the plane of its pass, mm, Y-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LeadArc {
    /// Where the arc starts: the staging point of a lead-in, the contour
    /// point of a lead-out.
    pub from: Point,
    /// Where it ends.
    pub to: Point,
    /// Its centre.
    pub centre: Point,
    /// Counter-clockwise viewed from +Z.
    pub ccw: bool,
    /// Its radius, mm.
    pub radius_mm: f64,
    /// Its sweep, degrees, in (0, 180].
    pub sweep_deg: f64,
}

impl LeadArc {
    /// The move kind that cuts it.
    pub(crate) fn kind(&self) -> MoveKind {
        if self.ccw {
            MoveKind::ArcCcw {
                centre: self.centre,
            }
        } else {
            MoveKind::ArcCw {
                centre: self.centre,
            }
        }
    }

    /// The unit direction of travel at `p` on the arc.
    #[cfg(test)]
    pub(crate) fn tangent_at(&self, p: Point) -> Vec2 {
        let t = (p - self.centre)
            .perp()
            .normalized()
            .unwrap_or(Vec2::new(1.0, 0.0));
        if self.ccw { t } else { -t }
    }

    /// The point `fraction` of the way along the arc from `from`.
    fn point_at(&self, fraction: f64) -> Point {
        let sign = if self.ccw { 1.0 } else { -1.0 };
        self.centre
            + rotate(
                self.from - self.centre,
                sign * fraction * self.sweep_deg.to_radians(),
            )
    }
}

/// A planned lead-in.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LeadIn {
    /// The arc, from the staging point to the arrival on the contour.
    pub arc: LeadArc,
    /// How far along the input contour the arrival is, mm.
    pub arrival_mm: f64,
    /// The contour re-seamed to start, and so end, at the arrival.
    pub seam: Polyline,
}

impl LeadIn {
    /// Where the descent lands.
    pub(crate) fn staging(&self) -> Point {
        self.arc.from
    }

    /// Where the lead joins the contour.
    pub(crate) fn arrival(&self) -> Point {
        self.arc.to
    }
}

/// A planned exit.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LeadOut {
    /// The overcut along the contour past the arrival, an open path; `None`
    /// when the overcut is 0.
    pub overcut: Option<Polyline>,
    /// The exit arc; `None` when `lead_out` is off or none fits.
    pub arc: Option<LeadArc>,
}

/// `v` turned `angle_rad` counter-clockwise.
fn rotate(v: Vec2, angle_rad: f64) -> Vec2 {
    let (s, c) = angle_rad.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// One segment of a closed path with where it starts along the path.
struct Edge {
    /// Index of the segment, and of the vertex it starts at.
    index: usize,
    segment: Segment,
    start_mm: f64,
    length_mm: f64,
}

/// The segments of `path` with their positions, and its length.
fn edges_of(path: &Polyline) -> (Vec<Edge>, f64) {
    let mut walked = 0.0;
    let edges = path
        .segments()
        .enumerate()
        .map(|(index, segment)| {
            let length_mm = segment.length_mm();
            let edge = Edge {
                index,
                segment,
                start_mm: walked,
                length_mm,
            };
            walked += length_mm;
            edge
        })
        .collect();
    (edges, walked)
}

/// The tangent of a non-degenerate segment at its start and end.
fn tangents(segment: &Segment) -> Option<(Vec2, Vec2)> {
    let length = segment.length_mm();
    if length <= LEAD_EPS {
        return None;
    }
    Some((segment.point_at(0.0)?.1, segment.point_at(length)?.1))
}

/// Upstream `turnScorer`: how much the contour turns within `window_mm`
/// either way of a position, in steps of [`LEAD_ARC_STEP_RAD`], counting
/// the turn at each vertex and, for an arc segment, the part of its sweep
/// inside the window.
struct TurnScorer {
    /// Each vertex's position and its turn, radians.
    vertices: Vec<(f64, f64)>,
    /// Each arc segment's start, length and absolute sweep.
    arcs: Vec<(f64, f64, f64)>,
    perimeter_mm: f64,
    window_mm: f64,
}

impl TurnScorer {
    fn new(edges: &[Edge], perimeter_mm: f64, window_mm: f64) -> Self {
        let live: Vec<(&Edge, (Vec2, Vec2))> = edges
            .iter()
            .filter_map(|e| tangents(&e.segment).map(|t| (e, t)))
            .collect();
        let mut vertices = Vec::with_capacity(live.len());
        for (k, (edge, (start, _))) in live.iter().enumerate() {
            let previous = live
                .get((k + live.len() - 1) % live.len().max(1))
                .map(|(_, (_, end))| *end);
            if let Some(incoming) = previous {
                let turn = incoming.cross(*start).atan2(incoming.dot(*start)).abs();
                if turn > LEAD_EPS {
                    vertices.push((edge.start_mm, turn));
                }
            }
        }
        let arcs = live
            .iter()
            .filter_map(|(e, _)| {
                e.segment
                    .arc()
                    .map(|a| (e.start_mm, e.length_mm, a.sweep_rad.abs()))
            })
            .collect();
        Self {
            vertices,
            arcs,
            perimeter_mm,
            window_mm,
        }
    }

    /// The quantised turn at `position_mm` along the path.
    fn at(&self, position_mm: f64) -> f64 {
        let p = self.perimeter_mm;
        let w = self.window_mm;
        let mut total = 0.0;
        for &(at, turn) in &self.vertices {
            let raw = (at - position_mm).abs().rem_euclid(p);
            if raw.min(p - raw) <= w {
                total += turn;
            }
        }
        for &(start, length, sweep) in &self.arcs {
            if length <= LEAD_EPS {
                continue;
            }
            let mut overlap = 0.0;
            for shift in [-p, 0.0, p] {
                let lo = start.max(position_mm - w + shift);
                let hi = (start + length).min(position_mm + w + shift);
                overlap += (hi - lo).max(0.0);
            }
            total += sweep * overlap.min(length) / length;
        }
        (total / LEAD_ARC_STEP_RAD).round()
    }
}

/// One position a lead may join a contour at (upstream `LeadArrival`).
#[derive(Debug, Clone, Copy)]
struct Arrival {
    /// The edge (segment index) it is on, and how far along, mm.
    edge: usize,
    local_mm: f64,
    fraction: f64,
    point: Point,
    /// The direction of travel there.
    tangent: Vec2,
    /// Distance from the path's start, mm.
    distance_mm: f64,
    /// Quantised turn within a diameter either way.
    corner: f64,
    /// Distance from where the tool comes from, mm.
    travel: f64,
}

/// Upstream `contourArrivals`: every position on `edges` best first,
/// ranked by corner turn, then travel from `from`, then edge, then
/// fraction.
fn arrivals(
    edges: &[Edge],
    perimeter_mm: f64,
    tool_diameter_mm: f64,
    from: Option<Point>,
) -> Vec<Arrival> {
    let scorer = TurnScorer::new(
        edges,
        perimeter_mm,
        tool_diameter_mm * LEAD_CORNER_WINDOW_DIAMETERS,
    );
    let mut out = Vec::new();
    for (k, &fraction) in LEAD_ARRIVAL_FRACTIONS.iter().enumerate() {
        for edge in edges {
            if edge.length_mm <= LEAD_EPS {
                continue;
            }
            let local_mm = edge.length_mm * fraction;
            let Some((point, tangent)) = edge.segment.point_at(local_mm) else {
                continue;
            };
            // At the edge's own vertex, exactly the vertex.
            let point = if k == 0 { edge.segment.start } else { point };
            let distance_mm = edge.start_mm + local_mm;
            out.push(Arrival {
                edge: edge.index,
                local_mm,
                fraction,
                point,
                tangent,
                distance_mm,
                corner: scorer.at(distance_mm),
                travel: from.map_or(0.0, |f| f.distance_to(point)),
            });
        }
    }
    out.sort_by(|a, b| {
        a.corner
            .total_cmp(&b.corner)
            .then(a.travel.total_cmp(&b.travel))
            .then(a.edge.cmp(&b.edge))
            .then(a.fraction.total_cmp(&b.fraction))
    });
    out
}

/// Upstream `orderedSides`: which way an arc may turn off the contour at
/// `anchor`, travelling along `tangent`: `1.0` left (counter-clockwise),
/// `-1.0` right. A probe either side answers; where both answer (open
/// space) both are offered, left first.
fn sides(anchor: Point, tangent: Vec2, tool_diameter_mm: f64, space: &dyn LeadSpace) -> Vec<f64> {
    let probe = tangent.perp() * (tool_diameter_mm * LEAD_SIDE_PROBE_FRACTION);
    let left = space.inside(anchor + probe);
    let right = space.inside(anchor + (-probe));
    match (left, right) {
        (true, false) => vec![1.0],
        (false, true) => vec![-1.0],
        (true, true) => vec![1.0, -1.0],
        (false, false) => Vec::new(),
    }
}

/// The arc of `radius_mm` and `sweep_deg` that arrives at `arrival`
/// travelling along `tangent`, turning to `side`.
fn arc_into(arrival: Point, tangent: Vec2, side: f64, radius_mm: f64, sweep_deg: f64) -> LeadArc {
    let centre = arrival + tangent.perp() * (side * radius_mm);
    let from = centre + rotate(arrival - centre, -side * sweep_deg.to_radians());
    LeadArc {
        from,
        to: arrival,
        centre,
        ccw: side > 0.0,
        radius_mm,
        sweep_deg,
    }
}

/// The arc of `radius_mm` and `sweep_deg` that leaves `departure`
/// travelling along `tangent`, turning to `side`.
fn arc_out(departure: Point, tangent: Vec2, side: f64, radius_mm: f64, sweep_deg: f64) -> LeadArc {
    let centre = departure + tangent.perp() * (side * radius_mm);
    let to = centre + rotate(departure - centre, side * sweep_deg.to_radians());
    LeadArc {
        from: departure,
        to,
        centre,
        ccw: side > 0.0,
        radius_mm,
        sweep_deg,
    }
}

/// The (radius, sweep) ladder for `lead`, widest radius first, then
/// widest sweep.
fn ladder(lead: &Lead) -> Vec<(f64, f64)> {
    let mut sweeps = vec![lead.sweep_deg];
    sweeps.extend(
        LEAD_SWEEP_FALLBACKS_DEG
            .iter()
            .copied()
            .filter(|s| *s < lead.sweep_deg - LEAD_EPS),
    );
    LEAD_RADIUS_FACTORS
        .iter()
        .map(|f| lead.radius_mm * f)
        .filter(|r| *r > LEAD_EPS)
        .flat_map(|r| sweeps.iter().map(move |s| (r, *s)))
        .collect()
}

/// Upstream `planXyLeadIn`: the lead-in onto the closed tool-centre
/// `path` for `lead` and a tool `tool_diameter_mm` across, in `space`,
/// the tool coming from `from` (`None`: travel does not rank). Radius
/// first, then sweep, then arrival, then side; the first arc that
/// [`lead_fits`] wins. `None` when nothing fits.
pub(crate) fn plan_lead_in(
    path: &Polyline,
    lead: &Lead,
    tool_diameter_mm: f64,
    space: &dyn LeadSpace,
    from: Option<Point>,
) -> Option<LeadIn> {
    let (edges, perimeter_mm) = edges_of(path);
    if perimeter_mm.is_nan() || perimeter_mm <= EPS_MM || !path.is_closed() {
        return None;
    }
    let ranked: Vec<(Arrival, Vec<f64>)> = arrivals(&edges, perimeter_mm, tool_diameter_mm, from)
        .into_iter()
        .map(|a| {
            let s = sides(a.point, a.tangent, tool_diameter_mm, space);
            (a, s)
        })
        .filter(|(_, s)| !s.is_empty())
        .collect();
    for (radius_mm, sweep_deg) in ladder(lead) {
        for (arrival, free) in &ranked {
            for &side in free {
                let arc = arc_into(arrival.point, arrival.tangent, side, radius_mm, sweep_deg);
                if lead_fits(arrival.point, &arc, space) {
                    return Some(LeadIn {
                        arc,
                        arrival_mm: arrival.distance_mm,
                        seam: reseam_at(path, arrival.edge, arrival.local_mm),
                    });
                }
            }
        }
    }
    None
}

/// Upstream `planXyLeadOut` with DreamCarve's overcut: from `seam` (the
/// contour as the lead-in re-seamed it, so the lap ends at its start),
/// continue `lead.overcut_mm` along it (at most one more lap), then the
/// mirror arc on the same ladder. With no overcut the arc departs along
/// the closing segment's tangent, the direction the lap ends in.
pub(crate) fn plan_lead_out(
    seam: &Polyline,
    lead: &Lead,
    tool_diameter_mm: f64,
    space: &dyn LeadSpace,
) -> LeadOut {
    let length_mm = seam.length_mm();
    let overcut_mm = lead.overcut_mm.min(length_mm);
    let (overcut, departure) = if overcut_mm > EPS_MM {
        let sub = sub_path(seam, overcut_mm);
        let end = sub
            .segments()
            .filter_map(|s| tangents(&s).map(|(_, t)| (s.end, t)))
            .last();
        (Some(sub), end)
    } else {
        let start = seam.vertices().first().map(|v| v.point());
        let closing = seam
            .segments()
            .filter_map(|s| tangents(&s).map(|(_, t)| t))
            .last();
        (None, start.zip(closing))
    };
    let arc = match departure {
        Some((point, tangent)) if lead.lead_out => {
            let free = sides(point, tangent, tool_diameter_mm, space);
            ladder(lead).into_iter().find_map(|(radius_mm, sweep_deg)| {
                free.iter()
                    .map(|&side| arc_out(point, tangent, side, radius_mm, sweep_deg))
                    .find(|arc| lead_fits(point, arc, space))
            })
        }
        _ => None,
    };
    LeadOut { overcut, arc }
}

/// The bulge of the part of a segment with bulge `bulge` that sweeps
/// `fraction` of it.
fn part_bulge(bulge: f64, fraction: f64) -> f64 {
    (bulge.atan() * fraction).tan()
}

/// `path`, closed, re-seamed to start `distance_mm` along it (modulo its
/// length): the same segments from a different start, the segment holding
/// the new start split in two there (arcs as two arcs). At a vertex,
/// nothing is split. Profile also re-seams a path off its tabs with it
/// (#176).
pub(crate) fn reseam(path: &Polyline, distance_mm: f64) -> Polyline {
    let (edges, perimeter_mm) = edges_of(path);
    if perimeter_mm.is_nan() || perimeter_mm <= EPS_MM {
        return path.clone();
    }
    let d = distance_mm.rem_euclid(perimeter_mm);
    edges
        .iter()
        .find(|e| d <= e.start_mm + e.length_mm)
        .map_or_else(
            || path.clone(),
            |e| reseam_at(path, e.index, d - e.start_mm),
        )
}

/// `path` re-seamed `local_mm` along its segment `index`.
fn reseam_at(path: &Polyline, index: usize, local_mm: f64) -> Polyline {
    let vertices = path.vertices();
    let n = vertices.len();
    let rotated = |k: usize| {
        let k = k % n.max(1);
        let mut out: Vec<Vertex> = vertices.get(k..).unwrap_or_default().to_vec();
        out.extend_from_slice(vertices.get(..k).unwrap_or_default());
        Polyline::new(out, true)
    };
    let Some(segment) = path.segments().nth(index) else {
        return path.clone();
    };
    let length_mm = segment.length_mm();
    if local_mm <= EPS_MM {
        return rotated(index);
    }
    if local_mm >= length_mm - EPS_MM {
        return rotated(index + 1);
    }
    let Some((split, _)) = segment.point_at(local_mm) else {
        return rotated(index);
    };
    let Some(&own) = vertices.get(index) else {
        return path.clone();
    };
    let fraction = local_mm / length_mm;
    let mut out = Vec::with_capacity(n + 1);
    out.push(Vertex::new(
        split.x,
        split.y,
        part_bulge(own.bulge, 1.0 - fraction),
    ));
    out.extend_from_slice(vertices.get(index + 1..).unwrap_or_default());
    out.extend_from_slice(vertices.get(..index).unwrap_or_default());
    out.push(Vertex::new(own.x, own.y, part_bulge(own.bulge, fraction)));
    Polyline::new(out, true)
}

/// The open path along the closed `path` from its start for `length_mm`
/// (the whole lap when that is its length or more).
pub(crate) fn sub_path(path: &Polyline, length_mm: f64) -> Polyline {
    let vertices = path.vertices();
    let mut out = Vec::new();
    let mut walked = 0.0;
    for (segment, &own) in path.segments().zip(vertices) {
        let length = segment.length_mm();
        if walked + length < length_mm - EPS_MM {
            out.push(own);
            walked += length;
            continue;
        }
        let local = (length_mm - walked).clamp(0.0, length);
        if local >= length - EPS_MM {
            out.push(own);
            out.push(Vertex::line(segment.end.x, segment.end.y));
        } else if local <= EPS_MM {
            out.push(Vertex::line(own.x, own.y));
        } else {
            out.push(Vertex::new(
                own.x,
                own.y,
                part_bulge(own.bulge, local / length),
            ));
            if let Some((end, _)) = segment.point_at(local) {
                out.push(Vertex::line(end.x, end.y));
            }
        }
        return Polyline::new(out, false);
    }
    // Rounding: the whole lap, back to the start.
    if let Some(first) = vertices.first() {
        out.push(Vertex::line(first.x, first.y));
    }
    Polyline::new(out, false)
}

/// The points `arc` is checked at, both ends included: angular steps
/// whose chord is at most `R · sin(step / 2) / 2`, a quarter of the
/// chord of upstream's 5° tessellation step (design spec, lead step 3),
/// so a spot between two of upstream's samples is still seen.
pub(crate) fn lead_samples(arc: &LeadArc) -> Vec<Point> {
    let sweep_rad = arc.sweep_deg.to_radians();
    let step_rad = 2.0 * ((LEAD_ARC_STEP_RAD / 2.0).sin() / 4.0).asin();
    let steps = (sweep_rad / step_rad).ceil().clamp(1.0, 1e5);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to [1, 1e5] above"
    )]
    let n = steps as u32;
    let mut out: Vec<Point> = (0..=n)
        .map(|k| arc.point_at(f64::from(k) / f64::from(n)))
        .collect();
    if let Some(first) = out.first_mut() {
        *first = arc.from;
    }
    if let Some(last) = out.last_mut() {
        *last = arc.to;
    }
    out
}

/// Upstream `validateLead`: true when `arc`, joining a contour at
/// `anchor` (its `to` for a lead-in, its `from` for a lead-out), is a real
/// lead that stays sound: its far end more than
/// [`MIN_LEAD_OFFSET_FRACTION`] of its radius from the anchor, and every
/// sample ([`lead_samples`]) within `sqrt(2 · R · safety)` of the anchor
/// [`LeadSpace::inside`], every one beyond [`LeadSpace::clear_of_walls`]
/// (where the arc has curved a safety clear of the wall it joins). The
/// far end is checked first: that is where a candidate usually fails.
pub(crate) fn lead_fits(anchor: Point, arc: &LeadArc, space: &dyn LeadSpace) -> bool {
    let far = if arc.to.distance_to(anchor) <= EPS_MM {
        arc.from
    } else {
        arc.to
    };
    let offset_mm = far.distance_to(anchor);
    if offset_mm.is_nan() || offset_mm <= arc.radius_mm * MIN_LEAD_OFFSET_FRACTION {
        return false;
    }
    let near_mm = (2.0 * arc.radius_mm * space.safety_mm()).sqrt();
    let ok = |p: Point| {
        if p.distance_to(anchor) <= near_mm {
            space.inside(p)
        } else {
            space.clear_of_walls(p)
        }
    };
    ok(far) && lead_samples(arc).into_iter().all(ok)
}

/// The radius more than 0, the sweep in (0, 180] degrees and the overcut
/// 0 or more that validation requires of operation `op`'s `lead`, checked
/// again: a lead outside them would cut something other than what
/// validation passed.
pub(crate) fn check(op: &OperationId, lead: &Lead) -> Result<(), CamError> {
    let invalid = |field, value, range| {
        Err(CamError::InvalidLead {
            op: op.clone(),
            field,
            value,
            range,
        })
    };
    if !(lead.radius_mm.is_finite() && lead.radius_mm > 0.0) {
        return invalid("radius_mm", lead.radius_mm, "more than 0");
    }
    if !(lead.sweep_deg.is_finite() && lead.sweep_deg > 0.0 && lead.sweep_deg <= 180.0) {
        return invalid("sweep_deg", lead.sweep_deg, "more than 0 and at most 180");
    }
    if !(lead.overcut_mm.is_finite() && lead.overcut_mm >= 0.0) {
        return invalid("overcut_mm", lead.overcut_mm, "0 or more");
    }
    Ok(())
}

/// Why a lead was not cut, as `lead_dropped`'s `reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropReason {
    /// No lead-in fits.
    NoRoom,
    /// No lead-out fits.
    Exit,
}

impl DropReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::NoRoom => "no_room",
            Self::Exit => "exit",
        }
    }
}

/// The lead warnings of one operation, one per shape and outcome however
/// many paths and passes of the shape had it: one `lead_shrunk` per shape
/// with the smallest radius and sweep cut, one `lead_dropped` per shape
/// and reason. In the order shapes first had them.
#[derive(Debug, Default)]
pub(crate) struct LeadWarnings {
    shrunk: Vec<(ShapeRef, f64, f64)>,
    dropped: Vec<(ShapeRef, DropReason)>,
}

impl LeadWarnings {
    /// Notes the size of `arc`, cut for `shape`.
    pub(crate) fn record_arc(&mut self, shape: &ShapeRef, lead: &Lead, arc: &LeadArc) {
        let smaller = arc.radius_mm < lead.radius_mm - LEAD_EPS * lead.radius_mm.max(1.0)
            || arc.sweep_deg < lead.sweep_deg - LEAD_EPS;
        if !smaller {
            return;
        }
        match self.shrunk.iter_mut().find(|(s, _, _)| s == shape) {
            Some((_, radius, sweep)) => {
                *radius = radius.min(arc.radius_mm);
                *sweep = sweep.min(arc.sweep_deg);
            }
            None => self
                .shrunk
                .push((shape.clone(), arc.radius_mm, arc.sweep_deg)),
        }
    }

    /// Notes that a lead on `shape` was not cut, for `reason`.
    pub(crate) fn record_dropped(&mut self, shape: &ShapeRef, reason: DropReason) {
        if !self.dropped.iter().any(|(s, r)| s == shape && *r == reason) {
            self.dropped.push((shape.clone(), reason));
        }
    }

    /// The warnings, each naming `op`, for `lead`.
    pub(crate) fn into_warnings(self, op: &OperationId, lead: &Lead) -> Vec<Warning> {
        let shrunk = self.shrunk.into_iter().map(|(shape, radius, sweep)| {
            shape_warning(WarningCode::LeadShrunk, &shape)
                .with("op", op)
                .with("requested_mm", lead.radius_mm)
                .with("used_mm", radius)
                .with("requested_sweep_deg", lead.sweep_deg)
                .with("used_sweep_deg", sweep)
        });
        let dropped = self.dropped.into_iter().map(|(shape, reason)| {
            shape_warning(WarningCode::LeadDropped, &shape)
                .with("op", op)
                .with("reason", reason.as_str())
        });
        shrunk.chain(dropped).collect()
    }
}

/// `lead_dropped` for an On line operation `op`, which has no waste side.
pub(crate) fn on_line_warning(op: &OperationId) -> Warning {
    Warning::new(WarningCode::LeadDropped)
        .with("op", op)
        .with("reason", "on_line")
}

#[cfg(test)]
mod tests;
