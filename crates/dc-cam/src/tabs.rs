// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/tabs.ts

//! Holding tabs on Profile passes (`docs/specs/phase-1/design.md`,
//! "Algorithms → Profile contour", step 7).
//!
//! PureCutCNC places tabs as rectangles in the world and grows each by the
//! tool radius before clipping the path against it (`toolCentreContours`,
//! `expandedTabFootprints`). DreamCarve places tabs by distance along the
//! tool-centre contour instead, so that growth becomes a span of
//! `width + diameter` along the path: the cutter's full width must clear
//! the tab. World-placed tabs arrive with the GUI in phase 5.
//!
//! A smooth tab leaves a triangular bridge: base `width`, peak `height`
//! above the cut floor (#66). For a flat bit of radius r the tool centre
//! holds the tab top for `2r` centred on the tab and ramps straight down
//! over `width / 2` on each side, so the span is still `width + 2r`.
//! PureCutCNC's raised cosine (`tabProfile.ts`) over the same span touches
//! the tab top at a single point, which leaves only about 60% of the height
//! under a 6.35 mm bit; it is not ported.

use crate::error::{CamError, ShapeRef};
use crate::toolpath::{Move, MoveKind};
use crate::warning::{Warning, WarningCode};
use dc_geometry::{EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Point3, Polyline, Segment};
use dc_project::{OperationId, TabPlacement, TabShape, TabSpec};

/// Most chords one arc piece of a smooth ramp is split into. Bounds the
/// work a tiny chord tolerance can cause.
const MAX_ARC_CHORDS: u32 = 1024;

/// Distances along a closed path of length `path_length_mm` at which tabs
/// are centred, each in `[0, path_length_mm)`, in placement order.
///
/// `Count(n)` spaces `n` tabs `L/n` apart starting at `L/(2n)`, so no tab
/// straddles the path start unless tabs overlap. `AtDistances` takes each
/// value modulo `L`. A non-positive `L` gives no tabs.
#[must_use]
pub fn tab_centres_mm(placement: &TabPlacement, path_length_mm: f64) -> Vec<f64> {
    if path_length_mm.is_nan() || path_length_mm <= EPS_MM {
        return Vec::new();
    }
    match placement {
        TabPlacement::Count { count } => {
            let n = f64::from(*count);
            (0..*count)
                .map(|k| path_length_mm * (2.0 * f64::from(k) + 1.0) / (2.0 * n))
                .collect()
        }
        TabPlacement::AtDistances { distances_mm } => distances_mm
            .iter()
            .map(|d| d.rem_euclid(path_length_mm))
            .collect(),
    }
}

/// The feed rates a lap is cut at, exactly as the user set them on the
/// tool.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LapRates {
    /// XY cutting feed, mm/min.
    pub feed_mm_min: f64,
    /// Descent feed, mm/min; also used for tab lifts.
    pub plunge_mm_min: f64,
}

/// One operation's tab settings, checked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tabs {
    placement: TabPlacement,
    shape: TabShape,
    top_z_mm: f64,
    /// Width of one tab as left on the part.
    width_mm: f64,
    /// Length of path one tab raises: tab width plus tool diameter.
    span_mm: f64,
    chord_tolerance_mm: f64,
}

impl Tabs {
    /// Checks `spec` for operation `op` cut with a tool `diameter_mm` wide.
    /// `chord_tolerance_mm` bounds how far the chords of a smooth ramp on an
    /// arc stray from the path in XY; a value that is not positive falls
    /// back to [`FLATTEN_TOLERANCE_DEFAULT_MM`].
    ///
    /// Fails with [`CamError::InvalidTab`] when the width or height is not
    /// a positive number or a distance is not finite.
    pub(crate) fn new(
        op: &OperationId,
        spec: &TabSpec,
        diameter_mm: f64,
        chord_tolerance_mm: f64,
    ) -> Result<Self, CamError> {
        let invalid = |field: String, value_mm: f64| CamError::InvalidTab {
            op: op.clone(),
            field,
            value_mm,
        };
        for (field, value_mm) in [("width_mm", spec.width_mm), ("height_mm", spec.height_mm)] {
            if !(value_mm.is_finite() && value_mm > 0.0) {
                return Err(invalid(field.to_owned(), value_mm));
            }
        }
        if let TabPlacement::AtDistances { distances_mm } = &spec.placement
            && let Some((i, d)) = distances_mm
                .iter()
                .enumerate()
                .find(|(_, d)| !d.is_finite())
        {
            return Err(invalid(format!("distances_mm[{i}]"), *d));
        }
        Ok(Self {
            placement: spec.placement.clone(),
            shape: spec.shape,
            top_z_mm: spec.height_mm,
            width_mm: spec.width_mm,
            span_mm: spec.width_mm + diameter_mm,
            chord_tolerance_mm: if chord_tolerance_mm > 0.0 {
                chord_tolerance_mm
            } else {
                FLATTEN_TOLERANCE_DEFAULT_MM
            },
        })
    }

    /// The raised spans on a closed path `path_length_mm` long, and whether
    /// any of them overlapped (or touched, or one alone covers the whole
    /// path), which merges them.
    ///
    /// Each tab raises `width + diameter` of path centred on its distance
    /// (PureCutCNC grows each tab footprint by the tool radius on every
    /// side; along the path that is the same length). Spans are circular:
    /// one may run through the path start.
    pub(crate) fn layout(&self, path_length_mm: f64) -> (PathTabs, bool) {
        let l = path_length_mm;
        let mut layout = PathTabs {
            length_mm: l,
            spans: Vec::new(),
            full: false,
        };
        let centres = tab_centres_mm(&self.placement, l);
        if centres.is_empty() {
            return (layout, false);
        }
        let half = self.span_mm / 2.0;
        let mut starts: Vec<f64> = centres.iter().map(|c| (c - half).rem_euclid(l)).collect();
        starts.sort_by(f64::total_cmp);
        let mut overlap = self.span_mm >= l - EPS_MM;

        // (start, end) with end possibly beyond L, merged in order.
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for start in starts {
            let end = start + self.span_mm;
            match merged.last_mut() {
                Some(last) if start <= last.1 + EPS_MM => {
                    overlap = true;
                    last.1 = last.1.max(end);
                }
                _ => merged.push((start, end)),
            }
        }
        // The last span may run past L into the first.
        while merged.len() > 1 {
            let (Some(&first), Some(&last)) = (merged.first(), merged.last()) else {
                break;
            };
            if last.1 - l < first.0 - EPS_MM {
                break;
            }
            overlap = true;
            merged.remove(0);
            if let Some(last) = merged.last_mut() {
                last.1 = last.1.max(first.1 + l);
            }
        }
        layout.full = merged.iter().any(|(s, e)| e - s >= l - EPS_MM);
        layout.spans = merged
            .into_iter()
            .map(|(start_mm, end)| Span {
                start_mm,
                length_mm: end - start_mm,
            })
            .collect();
        (layout, overlap)
    }

    /// The length of path a smooth ramp rises or falls over: half the tab
    /// width, so the bridge left is a triangle `width` wide at its base.
    fn ramp_mm(&self) -> f64 {
        self.width_mm / 2.0
    }
}

/// One raised stretch of a closed path, after merging.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    /// Distance along the path where the span starts, in `[0, L)`.
    start_mm: f64,
    /// Length of the span; `start_mm + length_mm` may exceed `L`.
    length_mm: f64,
}

/// The merged tab spans on one closed tool-centre path.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PathTabs {
    length_mm: f64,
    spans: Vec<Span>,
    /// The spans cover the whole path.
    full: bool,
}

/// Where a point along a tabbed path lies.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Zone {
    /// Outside every span: the pass Z.
    Clear,
    /// On a smooth ramp, `f` of the way up from the pass Z (0) to the tab
    /// top (1).
    Ramp(f64),
    /// At the tab top.
    Hold,
}

impl PathTabs {
    /// The zone at `distance_mm` along the path.
    fn zone(&self, tabs: &Tabs, distance_mm: f64) -> Zone {
        if self.full {
            return Zone::Hold;
        }
        let ramp_mm = tabs.ramp_mm();
        for span in &self.spans {
            let t = (distance_mm - span.start_mm).rem_euclid(self.length_mm);
            if t >= span.length_mm {
                continue;
            }
            return match tabs.shape {
                TabShape::Rect => Zone::Hold,
                // A smooth span rises over its first half tab width and
                // falls over its last, holding the top between: one bit
                // diameter on a single tab, longer on a merged span.
                TabShape::Smooth if t < ramp_mm => Zone::Ramp(t / ramp_mm),
                TabShape::Smooth if t > span.length_mm - ramp_mm => {
                    Zone::Ramp((span.length_mm - t) / ramp_mm)
                }
                TabShape::Smooth => Zone::Hold,
            };
        }
        Zone::Clear
    }

    /// Z at `distance_mm` along the path for a pass at `base_z_mm`.
    fn z_at(&self, tabs: &Tabs, distance_mm: f64, base_z_mm: f64) -> f64 {
        match self.zone(tabs, distance_mm) {
            Zone::Clear => base_z_mm,
            // Laps at or above the tab top never get here (`emit_lap`).
            Zone::Ramp(f) => base_z_mm + (tabs.top_z_mm - base_z_mm) * f,
            Zone::Hold => tabs.top_z_mm,
        }
    }

    /// Distances where the lap must have a vertex: span boundaries and,
    /// for smooth tabs, the ends of each straight ramp.
    fn breakpoints_mm(&self, tabs: &Tabs) -> Vec<f64> {
        if self.full {
            return Vec::new();
        }
        let ramp_mm = tabs.ramp_mm();
        let mut out = Vec::new();
        for span in &self.spans {
            let end = span.start_mm + span.length_mm;
            out.push(span.start_mm);
            out.push(end);
            if tabs.shape == TabShape::Smooth {
                out.push(span.start_mm + ramp_mm);
                out.push(end - ramp_mm);
            }
        }
        for d in &mut out {
            *d = d.rem_euclid(self.length_mm);
        }
        out
    }
}

/// The warnings about where the tab top lies relative to the cut
/// (PureCutCNC `applyTabWarnings`): [`WarningCode::TabsOutsideCutRange`]
/// when no pass of `pass_z_mm` lies below the tab top, so the tabs change
/// nothing, or when the tab top is above the stock top `thickness_mm`.
pub(crate) fn cut_range_warnings(
    op: &OperationId,
    tabs: &Tabs,
    pass_z_mm: &[f64],
    thickness_mm: f64,
) -> Vec<Warning> {
    let Some(deepest_z_mm) = pass_z_mm.iter().copied().reduce(f64::min) else {
        return Vec::new();
    };
    let warning = |reason: &str| {
        Warning::new(WarningCode::TabsOutsideCutRange)
            .with("op", op)
            .with("reason", reason)
            .with("tab_top_mm", tabs.top_z_mm)
            .with("deepest_pass_z_mm", deepest_z_mm)
            .with("stock_top_mm", thickness_mm)
    };
    let mut warnings = Vec::new();
    if tabs.top_z_mm > thickness_mm + EPS_MM {
        warnings.push(warning("above_stock_top"));
    }
    if deepest_z_mm >= tabs.top_z_mm - EPS_MM {
        warnings.push(warning("cut_stops_above_tab"));
    }
    warnings
}

/// A warning about the tabs on contour `contour` (0-based, in cut order)
/// of operation `op`, cut for `shape` when it is known.
fn contour_warning(
    code: WarningCode,
    op: &OperationId,
    contour: usize,
    shape: Option<&ShapeRef>,
) -> Warning {
    let warning = Warning::new(code).with("op", op).with("contour", contour);
    match shape {
        Some(shape) => warning
            .with("instance", &shape.instance)
            .with("instance_name", &shape.instance_name)
            .with("profile", shape.profile),
        None => warning,
    }
}

/// The warning that tab spans on contour `contour` (0-based, in cut order)
/// overlapped and were merged.
pub(crate) fn overlap_warning(
    op: &OperationId,
    contour: usize,
    shape: Option<&ShapeRef>,
    path_length_mm: f64,
) -> Warning {
    contour_warning(WarningCode::TabsOverlap, op, contour, shape)
        .with("path_length_mm", path_length_mm)
}

/// [`WarningCode::TabsCoverMostOfContour`] when the merged spans of
/// `layout` cover more than half of contour `contour` (0-based, in cut
/// order), so the part or hole may not cut free. The tabs are not changed:
/// the user may want them, and they never make motion unsafe.
pub(crate) fn coverage_warning(
    op: &OperationId,
    contour: usize,
    shape: Option<&ShapeRef>,
    tabs: &Tabs,
    layout: &PathTabs,
) -> Option<Warning> {
    let l = layout.length_mm;
    let covered_mm = if layout.full {
        l
    } else {
        layout.spans.iter().map(|s| s.length_mm).sum::<f64>().min(l)
    };
    let half_mm = l / 2.0;
    if covered_mm <= half_mm + EPS_MM {
        return None;
    }
    let centres = tab_centres_mm(&tabs.placement, l);
    let count_f: f64 = centres.iter().map(|_| 1.0).sum();
    // Evenly spaced tabs that fit in half the path never overlap.
    let max_count = ((half_mm + EPS_MM) / tabs.span_mm).floor();
    let diameter_mm = tabs.span_mm - tabs.width_mm;
    let mut warning = contour_warning(WarningCode::TabsCoverMostOfContour, op, contour, shape)
        .with("path_length_mm", l)
        .with("tab_count", centres.len())
        .with("tab_span_mm", tabs.span_mm)
        .with("tab_total_mm", tabs.span_mm * count_f)
        .with("max_count", max_count);
    // Rounded down to 0.01 mm so the suggested width does fit.
    let max_width_mm = ((half_mm / count_f - diameter_mm) * 100.0).floor() / 100.0;
    if max_width_mm > 0.0 {
        warning = warning.with("max_width_mm", max_width_mm);
    }
    Some(warning)
}

/// One move of a lap, before the vertical step that may precede it.
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// Z the move must start at.
    from_z_mm: f64,
    kind: MoveKind,
    to: Point3,
}

/// Appends one lap of `path` at `z_mm`: a vertical move from the tool's
/// current position (above the path start) to the lap's start Z, then the
/// lap back to the path start. Where `tabs` is given and `z_mm` lies below
/// the tab top, the lap rises over each tab span: rect tabs with a vertical
/// lift, a traverse at the tab top, and a vertical descent; smooth tabs
/// along straight ramps either side of a hold at the tab top. Every vertical
/// move is at the plunge rate; every
/// move along the path, smooth ramps down included, is at the feed rate.
pub(crate) fn emit_lap(
    moves: &mut Vec<Move>,
    path: &Polyline,
    z_mm: f64,
    tabs: Option<(&Tabs, &PathTabs)>,
    rates: LapRates,
) {
    let Some(start) = path.vertices().first().map(|v| v.point()) else {
        return;
    };
    let tabs = tabs.filter(|(t, _)| z_mm < t.top_z_mm - EPS_MM);
    let pieces = lap_pieces(path, z_mm, tabs);
    let start_z_mm = pieces.first().map_or(z_mm, |p| p.from_z_mm);
    let Some(mut current) = moves.last().map(|m| m.to) else {
        // A lap always follows the rapid to its start; without one there
        // is no safe way in.
        return;
    };
    let vertical_to = |moves: &mut Vec<Move>, current: &mut Point3, z: f64| {
        if (current.z - z).abs() > EPS_MM {
            *current = Point3::new(start.x, start.y, z);
            moves.push(Move::linear(*current, rates.plunge_mm_min));
        }
    };
    vertical_to(moves, &mut current, start_z_mm);
    for piece in pieces {
        if (current.z - piece.from_z_mm).abs() > EPS_MM {
            current = Point3::new(current.x, current.y, piece.from_z_mm);
            moves.push(Move::linear(current, rates.plunge_mm_min));
        }
        moves.push(Move {
            kind: piece.kind,
            to: piece.to,
            feed_mm_min: Some(rates.feed_mm_min),
        });
        current = piece.to;
    }
}

/// The moves of one lap at `z_mm`, split at tab breakpoints.
fn lap_pieces(path: &Polyline, z_mm: f64, tabs: Option<(&Tabs, &PathTabs)>) -> Vec<Piece> {
    let segments: Vec<Segment> = path
        .segments()
        .filter(|s| s.start.distance_to(s.end) > EPS_MM)
        .collect();
    let Some((tabs, layout)) = tabs else {
        return segments
            .iter()
            .map(|s| Piece {
                from_z_mm: z_mm,
                kind: segment_kind(s),
                to: Point3::new(s.end.x, s.end.y, z_mm),
            })
            .collect();
    };
    let mut breakpoints = layout.breakpoints_mm(tabs);
    breakpoints.sort_by(f64::total_cmp);

    let lap = Lap {
        tabs,
        layout,
        base_z_mm: z_mm,
    };
    let mut pieces = Vec::new();
    let mut segment_start_mm = 0.0;
    for segment in &segments {
        let length_mm = segment.length_mm();
        let mut local: Vec<f64> = vec![0.0];
        local.extend(
            breakpoints
                .iter()
                .map(|b| b - segment_start_mm)
                .filter(|d| *d > EPS_MM && *d < length_mm - EPS_MM),
        );
        local.push(length_mm);
        local.dedup_by(|b, a| *b - *a <= EPS_MM);
        for pair in local.windows(2) {
            if let &[a, b] = pair {
                lap.push_piece(&mut pieces, segment, segment_start_mm, a, b);
            }
        }
        segment_start_mm += length_mm;
    }
    pieces
}

/// One tabbed lap being split into pieces.
struct Lap<'a> {
    tabs: &'a Tabs,
    layout: &'a PathTabs,
    base_z_mm: f64,
}

impl Lap<'_> {
    /// Appends the piece of `segment` from local distance `a` to `b`, which
    /// lies in one zone. `segment_start_mm` is the segment's distance along
    /// the path.
    fn push_piece(
        &self,
        pieces: &mut Vec<Piece>,
        segment: &Segment,
        segment_start_mm: f64,
        a: f64,
        b: f64,
    ) {
        let mid = segment_start_mm + (a + b) / 2.0;
        let level_z_mm = match self.layout.zone(self.tabs, mid) {
            Zone::Clear => Some(self.base_z_mm),
            Zone::Hold => Some(self.tabs.top_z_mm),
            Zone::Ramp(_) => None,
        };
        if let Some(z) = level_z_mm {
            let end = point_on(segment, b);
            pieces.push(Piece {
                from_z_mm: z,
                kind: segment_kind(segment),
                to: Point3::new(end.x, end.y, z),
            });
            return;
        }
        // A ramp: Z is linear in distance along the path, so a straight
        // piece is one move; an arc is split into chords that stay within
        // the tolerance in XY, Z exact at each chord end.
        let chords = match segment.arc() {
            Some(arc) => {
                let angle_rad = (b - a) / arc.radius_mm;
                let tol = self.tabs.chord_tolerance_mm;
                let max_rad = if tol < arc.radius_mm {
                    2.0 * (1.0 - tol / arc.radius_mm).acos()
                } else {
                    std::f64::consts::PI
                };
                (1..MAX_ARC_CHORDS)
                    .find(|&k| angle_rad / f64::from(k) <= max_rad)
                    .unwrap_or(MAX_ARC_CHORDS)
            }
            None => 1,
        };
        let z_at = |d: f64| {
            self.layout
                .z_at(self.tabs, segment_start_mm + d, self.base_z_mm)
        };
        let at = |k: u32| {
            if k == chords {
                b
            } else {
                a + (b - a) * f64::from(k) / f64::from(chords)
            }
        };
        for k in 0..chords {
            let (d0, d1) = (at(k), at(k + 1));
            let end = point_on(segment, d1);
            pieces.push(Piece {
                from_z_mm: z_at(d0),
                kind: MoveKind::Linear,
                to: Point3::new(end.x, end.y, z_at(d1)),
            });
        }
    }
}

/// The point `distance_mm` along `segment`; exactly its end at its length.
fn point_on(segment: &Segment, distance_mm: f64) -> Point {
    if distance_mm >= segment.length_mm() - EPS_MM {
        return segment.end;
    }
    segment
        .point_at(distance_mm)
        .map_or(segment.end, |(p, _)| p)
}

/// How a whole segment, or a constant-Z piece of it, is cut. A positive
/// bulge, hence a positive sweep, is counter-clockwise in Y-up.
fn segment_kind(segment: &Segment) -> MoveKind {
    match segment.arc() {
        Some(arc) if arc.sweep_rad > 0.0 => MoveKind::ArcCcw { centre: arc.centre },
        Some(arc) => MoveKind::ArcCw { centre: arc.centre },
        None => MoveKind::Linear,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::generate_profile;
    use crate::test_support::assert_safe;
    use crate::toolpath::{MoveKind, Toolpath};
    use crate::warning::WarningCode;
    use dc_geometry::{EPS_MM, Point, Point3, Transform, assert_approx_eq};
    use dc_project::test_support::sample_project;
    use dc_project::{
        CutDirection, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Operation,
        ProfileOp, Project, Side, Stock, ToolId,
    };
    use std::collections::BTreeMap;
    use std::f64::consts::TAU;

    const TOL: f64 = 1e-6;
    /// Smooth ramps are cut as chords of the arc, so lengths summed over
    /// them fall short of the arc length by at most this here (chord
    /// tolerance 0.01 mm on a 48 mm radius).
    const CHORD_TOL: f64 = 1e-3;
    const THICKNESS_MM: f64 = 12.0;
    const DIAMETER_MM: f64 = 6.35;
    const TAB_WIDTH_MM: f64 = 8.0;
    const TAB_Z_MM: f64 = 3.0;
    const FEED_MM_MIN: f64 = 1000.0;
    const PLUNGE_MM_MIN: f64 = 300.0;
    /// Passes of the coaster: 12 mm deep in 4 mm steps.
    const PASS_Z_MM: [f64; 3] = [8.0, 4.0, 0.0];
    const COASTER_RADIUS_MM: f64 = 45.0;

    /// The spec's coaster (`docs/specs/phase-1/functional.md`, criterion
    /// 11): a 90 mm circle centred at (60, 60) on 12 mm stock, cut outside,
    /// climb, 12 mm deep in 4 mm steps with the 6.35 mm tool.
    fn coaster(tabs: Option<TabSpec>) -> (Project, ProfileOp) {
        circle_job(COASTER_RADIUS_MM, Side::Outside, tabs)
    }

    fn circle_job(radius_mm: f64, side: Side, tabs: Option<TabSpec>) -> (Project, ProfileOp) {
        job(
            "Coaster",
            vec![Polyline::circle(Point::new(0.0, 0.0), radius_mm)],
            side,
            tabs,
        )
    }

    /// A 60 mm square part with a centred square hole `hole_mm` wide, cut
    /// outside (so the hole is cut inside), hole first.
    fn square_with_hole(hole_mm: f64, tabs: Option<TabSpec>) -> (Project, ProfileOp) {
        let square = |side_mm: f64| {
            Polyline::rect(Point::new(-side_mm / 2.0, -side_mm / 2.0), side_mm, side_mm)
        };
        job(
            "Square",
            vec![square(60.0), square(hole_mm)],
            Side::Outside,
            tabs,
        )
    }

    /// One definition named `name` holding `profiles`, placed once at
    /// (60, 60) as instance `i1` named "`name` 1", cut by Profile `o1`.
    fn job(
        name: &str,
        profiles: Vec<Polyline>,
        side: Side,
        tabs: Option<TabSpec>,
    ) -> (Project, ProfileOp) {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 120.0,
            height_mm: 120.0,
            thickness_mm: Some(THICKNESS_MM),
        };
        let def = FeatureDefinition {
            id: DefinitionId::new("d1"),
            name: name.into(),
            profiles,
        };
        p.definitions = BTreeMap::from([(def.id.clone(), def)]);
        p.instances = vec![FeatureInstance {
            id: InstanceId::new("i1"),
            definition: DefinitionId::new("d1"),
            transform: Transform::translate(60.0, 60.0),
            name: format!("{name} 1"),
            visible: true,
            locked: false,
            z_top_mm: THICKNESS_MM,
            z_bottom_mm: 0.0,
        }];
        let op = ProfileOp {
            id: OperationId::new("o1"),
            name: "Cut out".into(),
            enabled: true,
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t1"),
            side,
            depth_mm: Some(THICKNESS_MM),
            step_down_mm: Some(4.0),
            direction: CutDirection::Climb,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs,
        };
        p.operations = vec![Operation::Profile(op.clone())];
        (p, op)
    }

    fn tab_spec(placement: TabPlacement, shape: TabShape) -> TabSpec {
        TabSpec {
            placement,
            width_mm: TAB_WIDTH_MM,
            height_mm: TAB_Z_MM,
            shape,
        }
    }

    fn four_rect_tabs() -> TabSpec {
        tab_spec(TabPlacement::Count { count: 4 }, TabShape::Rect)
    }

    /// Length of the coaster's tool-centre path.
    fn coaster_path_mm() -> f64 {
        TAU * (COASTER_RADIUS_MM + DIAMETER_MM / 2.0)
    }

    fn generate(project: &Project, op: &ProfileOp) -> (Toolpath, Vec<Warning>) {
        let (toolpath, warnings) = generate_profile(project, op).unwrap();
        assert_safe(&toolpath, project, op);
        (toolpath, warnings)
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code).collect()
    }

    /// A move that changes XY, with where it started.
    #[derive(Debug, Clone, Copy)]
    struct XyMove {
        from: Point3,
        mv: Move,
        /// Length along the path in XY, arcs measured as arcs.
        length_mm: f64,
    }

    impl XyMove {
        fn z_max(&self) -> f64 {
            self.from.z.max(self.mv.to.z)
        }

        fn at_z(&self, z_mm: f64) -> bool {
            (self.from.z - z_mm).abs() <= TOL && (self.mv.to.z - z_mm).abs() <= TOL
        }
    }

    /// Every non-rapid move that changes XY, in order.
    fn xy_moves(toolpath: &Toolpath) -> Vec<XyMove> {
        let mut out = Vec::new();
        let Some(first) = toolpath.moves.first() else {
            return out;
        };
        let mut pos = first.to;
        for mv in &toolpath.moves {
            let (a, b) = (pos.xy(), mv.to.xy());
            if !mv.is_rapid() && a.distance_to(b) > EPS_MM {
                let length_mm = match mv.kind {
                    MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                        let (u, v) = (a - centre, b - centre);
                        let mut sweep = u.cross(v).atan2(u.dot(v)).abs();
                        let ccw = matches!(mv.kind, MoveKind::ArcCcw { .. });
                        if (u.cross(v) < 0.0) == ccw {
                            sweep = TAU - sweep;
                        }
                        a.distance_to(centre) * sweep
                    }
                    _ => a.distance_to(b),
                };
                out.push(XyMove {
                    from: pos,
                    mv: *mv,
                    length_mm,
                });
            }
            pos = mv.to;
        }
        out
    }

    /// The XY moves of the final pass: every one that stays below the
    /// pass before it.
    fn final_lap(toolpath: &Toolpath) -> Vec<XyMove> {
        xy_moves(toolpath)
            .into_iter()
            .filter(|m| m.z_max() < PASS_Z_MM[1] - TOL)
            .collect()
    }

    /// A run of consecutive XY moves along a lap.
    #[derive(Debug, Clone, Copy)]
    struct Run {
        /// Distance along the lap from its start to the run's start.
        start_mm: f64,
        length_mm: f64,
    }

    /// The runs of `lap` whose moves satisfy `pred`; a run that wraps
    /// through the lap start is joined into one.
    fn runs(lap: &[XyMove], pred: impl Fn(&XyMove) -> bool) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        let mut at_mm = 0.0;
        let mut open = false;
        for m in lap {
            if pred(m) {
                if open {
                    runs.last_mut().unwrap().length_mm += m.length_mm;
                } else {
                    runs.push(Run {
                        start_mm: at_mm,
                        length_mm: m.length_mm,
                    });
                    open = true;
                }
            } else {
                open = false;
            }
            at_mm += m.length_mm;
        }
        let wraps = lap.first().is_some_and(&pred) && lap.last().is_some_and(&pred);
        if wraps && runs.len() > 1 {
            let first = runs.remove(0);
            runs.last_mut().unwrap().length_mm += first.length_mm;
        }
        runs
    }

    fn raised_runs(lap: &[XyMove]) -> Vec<Run> {
        runs(lap, |m| m.at_z(TAB_Z_MM))
    }

    /// After the descent into it, the final pass never moves Z alone: a
    /// smooth tab ramps rather than steps.
    fn assert_no_vertical_step_in_final_pass(tp: &Toolpath) {
        let final_start = tp
            .moves
            .iter()
            .position(|m| m.to.z < PASS_Z_MM[1] - TOL)
            .unwrap();
        for pair in tp.moves[final_start..].windows(2) {
            let (prev, mv) = (pair[0], pair[1]);
            let vertical = prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM;
            assert!(
                mv.is_rapid() || !vertical || (mv.to.z - prev.to.z).abs() <= TOL,
                "no vertical step inside the final pass: {prev:?} -> {mv:?}"
            );
        }
    }

    #[test]
    fn count_four_spaces_evenly() {
        let centres = tab_centres_mm(&TabPlacement::Count { count: 4 }, 100.0);
        assert_eq!(centres.len(), 4);
        for (c, e) in centres.iter().zip([12.5, 37.5, 62.5, 87.5]) {
            assert_approx_eq!(*c, e, TOL);
        }
        let at = TabPlacement::AtDistances {
            distances_mm: vec![-10.0, 110.0, 50.0],
        };
        let centres = tab_centres_mm(&at, 100.0);
        for (c, e) in centres.iter().zip([90.0, 10.0, 50.0]) {
            assert_approx_eq!(*c, e, TOL);
        }
        assert!(tab_centres_mm(&TabPlacement::Count { count: 0 }, 100.0).is_empty());

        // On the coaster, the raised runs are centred at L/8, 3L/8, 5L/8,
        // 7L/8 along the final lap.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (tp, _) = generate(&p, &op);
        let l = coaster_path_mm();
        let runs = raised_runs(&final_lap(&tp));
        assert_eq!(runs.len(), 4, "{runs:?}");
        for (k, run) in runs.iter().enumerate() {
            let centre = run.start_mm + run.length_mm / 2.0;
            assert_approx_eq!(centre, l * (2.0 * k as f64 + 1.0) / 8.0, TOL);
        }
    }

    #[test]
    fn span_is_width_plus_diameter() {
        // The tool-centre path, not the outline, is what tabs interrupt:
        // a 25.4 mm hole cut inside has a path π(25.4 − d) long (port of
        // "a circle's inside path is shorter than its bounding box
        // suggests").
        let width_mm = 7.9375;
        let small = |count| {
            circle_job(
                12.7,
                Side::Inside,
                Some(TabSpec {
                    placement: TabPlacement::Count { count },
                    width_mm,
                    height_mm: TAB_Z_MM,
                    shape: TabShape::Rect,
                }),
            )
        };
        let path_mm = std::f64::consts::PI * (25.4 - DIAMETER_MM);
        let span_mm = width_mm + DIAMETER_MM;

        // Two tabs leave most of the small circle free.
        let (p, op) = small(2);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        let total: f64 = lap.iter().map(|m| m.length_mm).sum();
        assert_approx_eq!(total, path_mm, TOL);
        let raised = raised_runs(&lap);
        assert_eq!(raised.len(), 2, "{raised:?}");
        for run in &raised {
            assert_approx_eq!(run.length_mm, span_mm, TOL);
        }

        // Four tabs all but cover it (port of "four tabs cover a circle"),
        // yet leave a gap to cut through, so nothing overlaps. They cover
        // most of the path, which warns but changes nothing.
        let (p, op) = small(4);
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::TabsCoverMostOfContour],
            "{warnings:?}"
        );
        let lap = final_lap(&tp);
        let free: f64 = runs(&lap, |m| m.at_z(0.0))
            .iter()
            .map(|r| r.length_mm)
            .sum();
        assert_approx_eq!(free, path_mm - 4.0 * span_mm, TOL);
        assert!(free / path_mm < 0.05 && free > 0.0, "free {free}");

        // And on the coaster each tab spans 8 + 6.35 mm.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (tp, _) = generate(&p, &op);
        for run in raised_runs(&final_lap(&tp)) {
            assert_approx_eq!(run.length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);
        }
    }

    #[test]
    fn passes_above_tab_height_untouched() {
        for (shape, top_z_mm) in [
            (TabShape::Rect, TAB_Z_MM),
            (TabShape::Smooth, TAB_Z_MM),
            // A pass exactly at the tab top is not below it.
            (TabShape::Rect, PASS_Z_MM[1]),
        ] {
            let mut spec = tab_spec(TabPlacement::Count { count: 4 }, shape);
            spec.height_mm = top_z_mm;
            let (p, op) = coaster(Some(spec));
            let (with_tabs, _) = generate(&p, &op);
            let (plain, _) = generate(&p, &ProfileOp { tabs: None, ..op });
            // Everything up to the descent into the final pass is identical.
            let final_descent = plain
                .moves
                .iter()
                .position(|m| m.to.z < PASS_Z_MM[1] - TOL)
                .unwrap();
            assert_eq!(
                with_tabs.moves[..final_descent],
                plain.moves[..final_descent],
                "{shape:?} at {top_z_mm}"
            );
            assert_ne!(with_tabs.moves, plain.moves, "the final pass is tabbed");
        }
    }

    #[test]
    fn final_pass_has_exactly_four_raised_segments_at_tab_z() {
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        let raised = raised_runs(&lap);
        assert_eq!(raised.len(), 4, "{raised:?}");
        for run in &raised {
            assert_approx_eq!(run.length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);
        }
        // Evenly spaced: a quarter of the path between run starts.
        let quarter = coaster_path_mm() / 4.0;
        for pair in raised.windows(2) {
            assert_approx_eq!(pair[1].start_mm - pair[0].start_mm, quarter, TOL);
        }
        // Every other XY move of the final pass is at the pass Z, and the
        // whole lap is accounted for.
        assert!(lap.iter().all(|m| m.at_z(0.0) || m.at_z(TAB_Z_MM)));
        let total: f64 = lap.iter().map(|m| m.length_mm).sum();
        assert_approx_eq!(total, coaster_path_mm(), TOL);

        // Each tab is a vertical lift at plunge feed, a traverse, and a
        // vertical descent at plunge feed.
        let final_start = tp
            .moves
            .iter()
            .position(|m| m.to.z < PASS_Z_MM[1] - TOL)
            .unwrap();
        let mut lifts = 0;
        let mut drops = 0;
        for pair in tp.moves[final_start..].windows(2) {
            let (prev, mv) = (pair[0], pair[1]);
            if mv.is_rapid() || (mv.to.z - prev.to.z).abs() <= TOL {
                continue;
            }
            assert!(
                prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM,
                "vertical: {mv:?}"
            );
            assert_eq!(mv.kind, MoveKind::Linear);
            assert_approx_eq!(mv.feed_mm_min.unwrap(), PLUNGE_MM_MIN, EPS_MM);
            if mv.to.z > prev.to.z {
                assert_approx_eq!(mv.to.z, TAB_Z_MM, TOL);
                lifts += 1;
            } else {
                drops += 1;
            }
        }
        assert_eq!(lifts, 4);
        assert_eq!(drops, 4, "one per tab");
        // Never below the stock bottom.
        assert!(tp.moves.iter().all(|m| m.to.z >= -EPS_MM));
    }

    #[test]
    fn tab_across_the_lap_start_never_plunges_into_it() {
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![0.0],
            },
            TabShape::Rect,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_approx_eq!(raised[0].length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);
        // The descent into the final pass stops at the tab top: the lap
        // starts inside the tab.
        let descent = tp
            .moves
            .iter()
            .find(|m| m.to.z < PASS_Z_MM[1] - TOL)
            .unwrap();
        assert_approx_eq!(descent.to.z, TAB_Z_MM, TOL);
    }

    #[test]
    fn overlapping_tabs_merge_and_warn() {
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![100.0, 108.0],
            },
            TabShape::Rect,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOverlap]);
        assert_eq!(warnings[0].param("op"), Some("o1"));
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 1, "merged: {raised:?}");
        assert_approx_eq!(raised[0].length_mm, 8.0 + TAB_WIDTH_MM + DIAMETER_MM, TOL);
        assert_approx_eq!(
            raised[0].start_mm,
            100.0 - (TAB_WIDTH_MM + DIAMETER_MM) / 2.0,
            TOL
        );

        // Two identical tabs overlap entirely: one span, still a warning.
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![100.0, 100.0],
            },
            TabShape::Rect,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOverlap]);
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 1);
        assert_approx_eq!(raised[0].length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);

        // Overlapping smooth tabs merge into one ramp that holds the top
        // between them.
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![100.0, 108.0],
            },
            TabShape::Smooth,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOverlap]);
        let lifted = runs(&final_lap(&tp), |m| m.z_max() > TOL);
        assert_eq!(lifted.len(), 1, "{lifted:?}");
        assert_approx_eq!(
            lifted[0].length_mm,
            8.0 + TAB_WIDTH_MM + DIAMETER_MM,
            CHORD_TOL
        );
        let held: f64 = raised_runs(&final_lap(&tp))
            .iter()
            .map(|r| r.length_mm)
            .sum();
        // From the first tab's one-diameter hold to the last's: the ramps
        // are half a tab width each.
        assert_approx_eq!(held, 8.0 + DIAMETER_MM, TOL);
        assert_no_vertical_step_in_final_pass(&tp);

        // Tabs wide enough to cover the whole path merge into one: the
        // final pass rides the tab top all the way round.
        let mut spec = four_rect_tabs();
        spec.width_mm = 80.0;
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(
            codes(&warnings),
            vec![
                WarningCode::TabsOverlap,
                WarningCode::TabsCoverMostOfContour
            ]
        );
        let lap = final_lap(&tp);
        assert!(lap.iter().all(|m| m.at_z(TAB_Z_MM)), "{lap:?}");

        // Spaced-out tabs do not warn.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (_, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// The XY moves of each contour's final pass, in cut order: a contour
    /// runs from one rapid to the next.
    fn final_laps_by_contour(toolpath: &Toolpath) -> Vec<Vec<XyMove>> {
        let mut out: Vec<Vec<XyMove>> = Vec::new();
        let mut current: Vec<XyMove> = Vec::new();
        let Some(first) = toolpath.moves.first() else {
            return out;
        };
        let mut pos = first.to;
        for mv in &toolpath.moves {
            if mv.is_rapid() {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            } else if pos.xy().distance_to(mv.to.xy()) > EPS_MM {
                let m = XyMove {
                    from: pos,
                    mv: *mv,
                    length_mm: 0.0,
                };
                if m.z_max() < PASS_Z_MM[1] - TOL {
                    current.push(m);
                }
            }
            pos = mv.to;
        }
        if !current.is_empty() {
            out.push(current);
        }
        out
    }

    fn coverage_warnings(warnings: &[Warning]) -> Vec<&Warning> {
        warnings
            .iter()
            .filter(|w| w.code == WarningCode::TabsCoverMostOfContour)
            .collect()
    }

    fn param_mm(warning: &Warning, key: &str) -> f64 {
        warning
            .param(key)
            .unwrap_or_else(|| panic!("{key} is set: {warning:?}"))
            .parse()
            .unwrap()
    }

    #[test]
    fn tabs_covering_most_of_contour_warn() {
        // Default tabs (4 × 8 mm, 6.35 mm bit) on a 20 mm square hole:
        // 57.4 mm of tab on a 54.6 mm tool path.
        let (p, op) = square_with_hole(20.0, Some(four_rect_tabs()));
        let (_, warnings) = generate(&p, &op);
        let cover = coverage_warnings(&warnings);
        assert_eq!(cover.len(), 1, "only the hole: {warnings:?}");
        let w = cover[0];
        assert_eq!(w.param("op"), Some("o1"));
        assert_eq!(w.param("contour"), Some("0"), "the hole is cut first");
        assert_eq!(w.param("instance"), Some("i1"));
        assert_eq!(w.param("instance_name"), Some("Square 1"));
        assert_eq!(w.param("profile"), Some("1"));
        assert_eq!(w.param("tab_count"), Some("4"));
        let path_mm = 4.0 * (20.0 - DIAMETER_MM);
        let span_mm = TAB_WIDTH_MM + DIAMETER_MM;
        assert_approx_eq!(param_mm(w, "path_length_mm"), path_mm, TOL);
        assert_approx_eq!(param_mm(w, "tab_span_mm"), span_mm, TOL);
        assert_approx_eq!(param_mm(w, "tab_total_mm"), 4.0 * span_mm, TOL);
        // One 14.35 mm tab covers at most half of 54.6 mm; four tabs fit
        // in half only if each is at most 27.3 / 4 − 6.35 = 0.475 mm wide.
        assert_eq!(w.param("max_count"), Some("1"));
        assert!(param_mm(w, "max_width_mm") <= path_mm / 2.0 / 4.0 - DIAMETER_MM);

        // The text names the shape, the path, the tab total and the share,
        // says what may happen, and suggests changes, in rounded values.
        let text = w.to_string();
        for needle in [
            "`Square 1`",
            "54.6 mm",
            "57.4 mm",
            "105.1%",
            "may not cut free",
            "at most 1 tab",
            "0.47 mm",
        ] {
            assert!(text.contains(needle), "{needle:?} in {text:?}");
        }
        assert!(!text.contains('='), "{text}");

        // The outer contour's tabs cover a fifth of it: no warning. A
        // larger hole is fine too.
        let (p, op) = square_with_hole(40.0, Some(four_rect_tabs()));
        let (_, warnings) = generate(&p, &op);
        assert!(coverage_warnings(&warnings).is_empty(), "{warnings:?}");
    }

    /// The coverage warning for four 3 mm tabs cut with a 1/8" (3.175 mm)
    /// bit on a 13.175 mm square hole: a 40 mm tool path, 24.7 mm of it
    /// raised.
    fn small_bit_coverage_warning() -> Warning {
        let spec = TabSpec {
            width_mm: 3.0,
            ..four_rect_tabs()
        };
        let (mut p, op) = square_with_hole(13.175, Some(spec));
        p.tools[0].diameter_mm = 3.175;
        let (_, warnings) = generate(&p, &op);
        let cover = coverage_warnings(&warnings);
        assert_eq!(cover.len(), 1, "only the hole: {warnings:?}");
        cover[0].clone()
    }

    #[test]
    fn coverage_warning_carries_tab_width_and_diameter() {
        let w = small_bit_coverage_warning();
        assert_approx_eq!(param_mm(&w, "tab_width_mm"), 3.0, TOL);
        assert_approx_eq!(param_mm(&w, "diameter_mm"), 3.175, TOL);
        assert_approx_eq!(param_mm(&w, "tab_span_mm"), 6.175, TOL);
        assert_approx_eq!(param_mm(&w, "tab_total_mm"), 4.0 * 6.175, TOL);
        assert_approx_eq!(param_mm(&w, "path_length_mm"), 40.0, TOL);
    }

    #[test]
    fn coverage_warning_text_names_the_tab_width() {
        let text = small_bit_coverage_warning().to_string();
        for needle in ["3 mm wide", "3.175 mm bit", "6.17 mm", "24.7 mm", "40 mm"] {
            assert!(text.contains(needle), "{needle:?} in {text:?}");
        }
        assert!(!text.contains("tab(s) of 6.17 mm each"), "{text}");
    }

    #[test]
    fn tabs_are_not_changed_when_they_warn() {
        // Eleven 14.35 mm spans on the 302.7 mm coaster path cover 52%:
        // a warning, and still eleven tabs exactly as set.
        let spec = tab_spec(TabPlacement::Count { count: 11 }, TabShape::Rect);
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::TabsCoverMostOfContour],
            "{warnings:?}"
        );
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 11, "{raised:?}");
        for run in &raised {
            assert_approx_eq!(run.length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);
        }

        // Tabs that cover a whole hole are kept too: its final lap rides
        // the tab top all the way round.
        let (p, op) = square_with_hole(20.0, Some(four_rect_tabs()));
        let (tp, _) = generate(&p, &op);
        let laps = final_laps_by_contour(&tp);
        assert_eq!(laps.len(), 2);
        assert!(laps[0].iter().all(|m| m.at_z(TAB_Z_MM)), "{:?}", laps[0]);
    }

    #[test]
    fn tabs_covering_more_than_half_warn() {
        // The boundary is more than half: exactly half does not warn.
        let l = coaster_path_mm();
        let one_tab = |span_mm: f64| {
            let mut spec = tab_spec(
                TabPlacement::AtDistances {
                    distances_mm: vec![100.0],
                },
                TabShape::Rect,
            );
            spec.width_mm = span_mm - DIAMETER_MM;
            coaster(Some(spec))
        };
        let (p, op) = one_tab(l / 2.0);
        let (_, warnings) = generate(&p, &op);
        assert!(coverage_warnings(&warnings).is_empty(), "{warnings:?}");
        let (p, op) = one_tab(l / 2.0 + 0.01);
        let (_, warnings) = generate(&p, &op);
        assert_eq!(coverage_warnings(&warnings).len(), 1, "{warnings:?}");

        // By count: 10 spans (47%) do not warn, 11 (52%) do.
        for (count, warns) in [(10, false), (11, true)] {
            let spec = tab_spec(TabPlacement::Count { count }, TabShape::Rect);
            let (p, op) = coaster(Some(spec));
            let (_, warnings) = generate(&p, &op);
            assert_eq!(!coverage_warnings(&warnings).is_empty(), warns, "{count}");
        }
    }

    #[test]
    fn final_pass_reaches_full_depth_between_tabs() {
        // Every contour's last lap reaches the cut depth somewhere, unless
        // its tabs cover most of it and say so.
        let mut wide = four_rect_tabs();
        wide.width_mm = 80.0;
        let jobs = [
            coaster(Some(four_rect_tabs())),
            coaster(Some(tab_spec(
                TabPlacement::Count { count: 4 },
                TabShape::Smooth,
            ))),
            coaster(Some(tab_spec(
                TabPlacement::Count { count: 11 },
                TabShape::Rect,
            ))),
            coaster(Some(tab_spec(
                TabPlacement::AtDistances {
                    distances_mm: vec![0.0],
                },
                TabShape::Rect,
            ))),
            coaster(Some(wide)),
            circle_job(12.7, Side::Inside, Some(four_rect_tabs())),
            square_with_hole(20.0, Some(four_rect_tabs())),
            square_with_hole(40.0, Some(four_rect_tabs())),
        ];
        for (p, op) in &jobs {
            let (tp, warnings) = generate(p, op);
            let laps = final_laps_by_contour(&tp);
            assert!(!laps.is_empty());
            for (i, lap) in laps.iter().enumerate() {
                if lap.iter().any(|m| m.at_z(0.0)) {
                    continue;
                }
                let index = i.to_string();
                assert!(
                    coverage_warnings(&warnings)
                        .iter()
                        .any(|w| w.param("contour") == Some(index.as_str())),
                    "contour {i} never reaches depth and nothing says so: {warnings:?}"
                );
            }
        }
    }

    /// A 100 × 40 mm bar cut outside, with one tab per `tabs`.
    fn bar(tabs: Option<TabSpec>) -> (Project, ProfileOp) {
        job(
            "Bar",
            vec![Polyline::rect(Point::new(-50.0, -20.0), 100.0, 40.0)],
            Side::Outside,
            tabs,
        )
    }

    /// One smooth tab centred `BAR_TAB_MM` along the bar's tool-centre
    /// path: the middle of a long straight edge.
    const BAR_TAB_MM: f64 = 60.0;

    fn bar_smooth_tab() -> (Project, ProfileOp) {
        bar(Some(tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![BAR_TAB_MM],
            },
            TabShape::Smooth,
        )))
    }

    /// The final lap as a Z profile along the path: (distance, Z) at each
    /// vertex, Z linear in distance between them.
    fn lap_profile(lap: &[XyMove]) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        let mut at_mm = 0.0;
        for m in lap {
            out.push((at_mm, m.from.z));
            at_mm += m.length_mm;
            out.push((at_mm, m.mv.to.z));
        }
        out
    }

    /// The material a flat bit of radius `DIAMETER_MM / 2` leaves above the
    /// stock bottom at `x_mm` along the path: the lowest the tool's
    /// bottom reaches over every centre position within one radius.
    fn material_mm(profile: &[(f64, f64)], x_mm: f64) -> f64 {
        let r = DIAMETER_MM / 2.0;
        let (lo, hi) = (x_mm - r, x_mm + r);
        let z_at = |s: f64| {
            profile
                .windows(2)
                .find(|w| w[0].0 <= s && s <= w[1].0 && w[1].0 > w[0].0)
                .map(|w| w[0].1 + (w[1].1 - w[0].1) * (s - w[0].0) / (w[1].0 - w[0].0))
                .unwrap()
        };
        profile
            .iter()
            .filter(|(s, _)| (lo..=hi).contains(s))
            .map(|(_, z)| *z)
            .chain([z_at(lo), z_at(hi)])
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn smooth_tab_plateau_is_bit_diameter() {
        let (p, op) = bar_smooth_tab();
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        // The tab sits on a straight edge.
        let lifted = runs(&lap, |m| m.z_max() > TOL);
        assert_eq!(lifted.len(), 1, "{lifted:?}");
        let mut at_mm = 0.0;
        for m in &lap {
            let inside = at_mm >= lifted[0].start_mm - TOL
                && at_mm + m.length_mm <= lifted[0].start_mm + lifted[0].length_mm + TOL;
            if inside {
                assert_eq!(m.mv.kind, MoveKind::Linear, "{m:?}");
            }
            at_mm += m.length_mm;
        }
        // The final pass holds the tab top for at least one bit diameter,
        // centred on the tab.
        let raised = raised_runs(&lap);
        assert_eq!(raised.len(), 1, "{raised:?}");
        assert!(
            raised[0].length_mm >= DIAMETER_MM - TOL,
            "plateau {} < {DIAMETER_MM}",
            raised[0].length_mm
        );
        assert_approx_eq!(
            raised[0].start_mm + raised[0].length_mm / 2.0,
            BAR_TAB_MM,
            TOL
        );
    }

    #[test]
    fn smooth_tab_bridge_is_full_height() {
        let (p, op) = bar_smooth_tab();
        let (tp, _) = generate(&p, &op);
        let profile = lap_profile(&final_lap(&tp));
        let peak = (-400..=400)
            .map(|i| material_mm(&profile, BAR_TAB_MM + f64::from(i) * 0.01))
            .fold(f64::NEG_INFINITY, f64::max);
        assert_approx_eq!(peak, TAB_Z_MM, 0.01);
    }

    #[test]
    fn smooth_tab_bridge_is_triangle() {
        let half_mm = TAB_WIDTH_MM / 2.0;
        let triangle = |x: f64| TAB_Z_MM * (1.0 - x.abs() / half_mm).max(0.0);
        let samples = [0.0, 1.0, -1.0, 2.0, -2.5, 3.0, -3.5, 4.0, -4.0, 5.0, -6.0];

        // On a straight edge.
        let (p, op) = bar_smooth_tab();
        let (tp, _) = generate(&p, &op);
        let profile = lap_profile(&final_lap(&tp));
        for x in samples {
            assert_approx_eq!(material_mm(&profile, BAR_TAB_MM + x), triangle(x), 0.01);
        }

        // On the coaster's arcs: four tabs, each the same triangle centred
        // on its distance, each lifted over width + diameter, every move
        // along the path at the feed rate, with no vertical step.
        let (p, op) = coaster(Some(tab_spec(
            TabPlacement::Count { count: 4 },
            TabShape::Smooth,
        )));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        let lifted = runs(&lap, |m| m.z_max() > TOL);
        assert_eq!(lifted.len(), 4, "{lifted:?}");
        for run in &lifted {
            assert_approx_eq!(run.length_mm, TAB_WIDTH_MM + DIAMETER_MM, CHORD_TOL);
        }
        let profile = lap_profile(&lap);
        let l = coaster_path_mm();
        for k in 0..4 {
            let centre = l * (2.0 * f64::from(k) + 1.0) / 8.0;
            for x in samples {
                assert_approx_eq!(material_mm(&profile, centre + x), triangle(x), 0.01);
            }
        }
        assert!(
            lap.iter()
                .all(|m| m.from.z >= -1e-12 && m.mv.to.z <= TAB_Z_MM + 1e-12)
        );
        for m in &lap {
            assert_approx_eq!(m.mv.feed_mm_min.unwrap(), FEED_MM_MIN, EPS_MM);
        }
        assert_no_vertical_step_in_final_pass(&tp);
    }

    #[test]
    fn tabs_above_cut_depth_warn() {
        // The cut stops at Z 6, above a 3 mm tab: the tabs never apply.
        let (p, mut op) = coaster(Some(four_rect_tabs()));
        op.depth_mm = Some(6.0);
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOutsideCutRange]);
        assert_eq!(warnings[0].param("op"), Some("o1"));
        assert_eq!(warnings[0].param("reason"), Some("cut_stops_above_tab"));
        let (plain, _) = generate(&p, &ProfileOp { tabs: None, ..op });
        assert_eq!(tp, plain, "tabs out of reach change nothing");

        // A tab top above the stock top is reported, and still applied to
        // every pass, in the air above the stock.
        let mut spec = four_rect_tabs();
        spec.height_mm = THICKNESS_MM + 2.0;
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOutsideCutRange]);
        assert_eq!(warnings[0].param("reason"), Some("above_stock_top"));
        let raised = runs(&xy_moves(&tp), |m| m.at_z(THICKNESS_MM + 2.0));
        assert_eq!(raised.len(), 4 * PASS_Z_MM.len());

        // Tabs within the cut do not warn.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (_, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn invalid_tab_spec_is_error() {
        for (width_mm, height_mm) in [(0.0, 3.0), (-1.0, 3.0), (8.0, 0.0), (f64::NAN, 3.0)] {
            let mut spec = four_rect_tabs();
            spec.width_mm = width_mm;
            spec.height_mm = height_mm;
            let (p, op) = coaster(Some(spec));
            let err = generate_profile(&p, &op).unwrap_err();
            assert!(matches!(err, CamError::InvalidTab { .. }), "{err:?}");
        }
        let (p, op) = coaster(Some(tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![f64::INFINITY],
            },
            TabShape::Rect,
        )));
        let err = generate_profile(&p, &op).unwrap_err();
        assert!(matches!(err, CamError::InvalidTab { .. }), "{err:?}");
    }
}
