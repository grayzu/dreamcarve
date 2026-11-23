// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/sketch/snapping.ts
//   src/components/canvas/snappingHelpers.ts
//   src/components/canvas/snappingHelpers.test.ts

//! Snapping (`docs/specs/phase-5/design.md`, "Algorithms → Snapping";
//! functional capabilities 82 and 82a): the point a drawing or editing
//! tool takes instead of the pointer's, when geometry or the grid is
//! near.
//!
//! [`resolve`] gathers the candidates within the snap radius of the
//! pointer and takes the best by rank, then by distance: an endpoint, a
//! midpoint, a centre and an intersection first; then a perpendicular
//! foot; then the nearest point of an outline; then the grid. The grid
//! has no reach test (Graphite's rule): with grid snapping on and
//! nothing better in reach, the point is on the grid.
//!
//! Changed from upstream:
//!
//! - **No snap gate.** Upstream discards a click when snapping is on and
//!   nothing snapped; here the point is the pointer's.
//! - **Targets are the outlines as drawn.** Upstream offers a canned
//!   shape's frame; here a slot's arcs, a rounded corner and a gear's
//!   teeth all snap, since a shape's outline is its `profiles`.
//! - **Suspend and constrain.** `Ctrl`/`⌘` held returns the pointer's
//!   point. `Shift` holds the point to 15° steps about the reference
//!   point (Graphite; upstream cycles an axis lock on an `Alt` tap), and
//!   a candidate on that ray still wins.
//! - **The grid's spacing follows the zoom**, or is the user's; upstream
//!   snaps to a fixed increment of the project.
//! - Intersection is on by default (the roadmap lists it among the
//!   object snaps); upstream has it opt-in. Dimension anchors, which a
//!   snap carries upstream so a constraint can follow geometry, are not
//!   ported: phase 5 has no constraints.
//!
//! Re-derived for Y-up: nothing here depends on direction. Every
//! candidate is a point of the geometry and every test a distance.

use std::collections::BTreeSet;

use dc_geometry::{Point, Polyline, Rect, Segment, Vec2, as_circle, crossings};
use dc_project::{InstanceId, Stock, Units};

use crate::scene::Scene;

/// The snap radius until the user changes it, logical pixels
/// (`DEFAULT_SNAP_SETTINGS.pixelRadius`).
pub const DEFAULT_RADIUS_PX: f64 = 14.0;
/// The least the snap radius can be, logical pixels.
pub const MIN_RADIUS_PX: f64 = 6.0;
/// The most the snap radius can be, logical pixels.
pub const MAX_RADIUS_PX: f64 = 32.0;

/// The steps `Shift` holds a direction to, degrees: the step phase 4's
/// rotation handle snaps to.
pub const ANGLE_STEP_DEG: f64 = 15.0;

/// The least on-screen spacing of the automatic snap grid, logical
/// pixels. The grid's steps run 1, 2, 5, 10, …, so its spacing is from
/// this to two and a half times it, and no point of the Viewport is more
/// than the default snap radius from a grid point.
const MIN_GRID_SPACING_PX: f64 = 8.0;

/// How far off the `Shift` ray a candidate may be and still be on it,
/// logical pixels.
const ON_RAY_PX: f64 = 0.5;

/// How round a closed outline of straight segments must be for its
/// centre to snap, mm ([`as_circle`]'s tolerance): a circle flattened at
/// import.
const CIRCLE_TOLERANCE_MM: f64 = 0.05;

/// What a point snapped to. In the order the View menu lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SnapKind {
    /// A vertex of an outline, a corner of the stock, or the origin
    /// (upstream's `point`).
    Endpoint,
    /// The middle of a line or an arc.
    Midpoint,
    /// The centre of an arc, a circle or a parametric shape.
    Centre,
    /// Where two outlines, or two parts of one, cross.
    Intersection,
    /// The nearest point of an outline (upstream's `line`).
    OnOutline,
    /// The foot of the perpendicular from the tool's last point.
    Perpendicular,
    /// A point of the grid.
    Grid,
}

impl SnapKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 7] = [
        Self::Endpoint,
        Self::Midpoint,
        Self::Centre,
        Self::Intersection,
        Self::OnOutline,
        Self::Perpendicular,
        Self::Grid,
    ];

    /// The kind's name in `gui.toml`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Endpoint => "endpoint",
            Self::Midpoint => "midpoint",
            Self::Centre => "centre",
            Self::Intersection => "intersection",
            Self::OnOutline => "on_outline",
            Self::Perpendicular => "perpendicular",
            Self::Grid => "grid",
        }
    }

    /// The kind named `key` in `gui.toml`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }

    /// Which candidates beat which (`snapPriority`): 1 wins.
    const fn rank(self) -> u8 {
        match self {
            Self::Endpoint | Self::Midpoint | Self::Centre | Self::Intersection => 1,
            Self::Perpendicular => 2,
            Self::OnOutline => 3,
            Self::Grid => 4,
        }
    }
}

/// The snapping preferences (`SnapSettings`): an app preference, never
/// project data.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapSettings {
    /// The master switch.
    pub enabled: bool,
    /// The kinds that are on.
    pub kinds: BTreeSet<SnapKind>,
    /// How near the pointer a candidate must be, logical pixels, from
    /// [`MIN_RADIUS_PX`] to [`MAX_RADIUS_PX`].
    pub radius_px: f64,
}

impl Default for SnapSettings {
    /// Everything on but Perpendicular, at 14 px.
    fn default() -> Self {
        Self {
            enabled: true,
            kinds: SnapKind::ALL
                .into_iter()
                .filter(|kind| *kind != SnapKind::Perpendicular)
                .collect(),
            radius_px: DEFAULT_RADIUS_PX,
        }
    }
}

impl SnapSettings {
    /// These settings with the radius held to its range
    /// (`normalizeSnapSettings`); a radius that is not a number is the
    /// default.
    #[must_use]
    pub fn normalized(mut self) -> Self {
        self.radius_px = if self.radius_px.is_finite() {
            self.radius_px.clamp(MIN_RADIUS_PX, MAX_RADIUS_PX)
        } else {
            DEFAULT_RADIUS_PX
        };
        self
    }

    /// Whether `kind` is offered.
    #[must_use]
    pub fn offers(&self, kind: SnapKind) -> bool {
        self.enabled && self.kinds.contains(&kind)
    }

    /// Turns `kind` on or off.
    pub fn set(&mut self, kind: SnapKind, on: bool) {
        if on {
            self.kinds.insert(kind);
        } else {
            self.kinds.remove(&kind);
        }
    }
}

/// Where the pointer is and what is held with it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pointer {
    /// The pointer in project space, mm.
    pub at_mm: Point,
    /// `Shift`: hold the direction from the reference point to 15° steps;
    /// on a box tool, make it square.
    pub shift: bool,
    /// `Alt`: on a box tool, draw from the centre; on a handle, move it
    /// alone.
    pub alt: bool,
    /// `Ctrl`, or `⌘` on a Mac: no snapping while it is held.
    pub command: bool,
}

impl Pointer {
    /// A pointer at `at_mm` with nothing held.
    #[must_use]
    pub const fn at(at_mm: Point) -> Self {
        Self {
            at_mm,
            shift: false,
            alt: false,
            command: false,
        }
    }
}

/// What there is to snap to.
#[derive(Debug, Clone, Copy)]
pub struct Targets<'a> {
    /// The visible shapes.
    pub scene: &'a Scene,
    /// The stock, whose outline and corners snap.
    pub stock: &'a Stock,
    /// The work zero in project space, mm, which snaps as an endpoint.
    pub origin: Point,
    /// The Viewport's zoom, logical pixels per millimetre.
    pub px_per_mm: f64,
    /// The snap grid's spacing, mm ([`grid_step_mm`]); `None` while the
    /// grid is hidden, which does not snap.
    pub grid_step_mm: Option<f64>,
    /// Shapes that are no target: the one being dragged or edited.
    pub skip: &'a [InstanceId],
}

/// The point a tool takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapped {
    /// The point, mm.
    pub at_mm: Point,
    /// What it snapped to; `None` when it is the pointer's own point
    /// (held to its `Shift` ray, when `Shift` is down).
    pub kind: Option<SnapKind>,
    /// A dashed line to draw with the marker: from the pointer to the
    /// outline for [`SnapKind::OnOutline`], from the reference point to
    /// the foot for [`SnapKind::Perpendicular`].
    pub guide: Option<(Point, Point)>,
}

impl Snapped {
    /// The pointer's own point: nothing snapped.
    #[must_use]
    pub const fn free(at_mm: Point) -> Self {
        Self {
            at_mm,
            kind: None,
            guide: None,
        }
    }
}

/// One point a snap could take.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    kind: SnapKind,
    at: Point,
    distance_mm: f64,
    guide: Option<(Point, Point)>,
}

/// The point the tool takes for `pointer` (`resolveSketchSnap`).
/// `reference` is the tool's last point: the perpendicular foot is
/// dropped from it, and `Shift` turns about it.
#[must_use]
pub fn resolve(
    targets: &Targets<'_>,
    settings: &SnapSettings,
    pointer: &Pointer,
    reference: Option<Point>,
) -> Snapped {
    // `Shift` first: the ray the point is held to, and the pointer on it.
    let ray = reference
        .filter(|_| pointer.shift)
        .and_then(|from| Ray::through(from, pointer.at_mm));
    let raw = ray.map_or(pointer.at_mm, |ray| ray.at);
    if !settings.enabled || pointer.command || !is_positive(targets.px_per_mm) {
        return Snapped::free(raw);
    }
    let radius_mm = settings.radius_px / targets.px_per_mm;
    let on_ray_mm = ON_RAY_PX / targets.px_per_mm;
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut offer = |kind: SnapKind, at: Point, guide: Option<(Point, Point)>| {
        if !settings.kinds.contains(&kind) || !at.x.is_finite() || !at.y.is_finite() {
            return;
        }
        let distance_mm = raw.distance_to(at);
        let on_ray = ray.is_none_or(|ray| ray.distance_mm(at) <= on_ray_mm);
        if distance_mm <= radius_mm && on_ray {
            candidates.push(Candidate {
                kind,
                at,
                distance_mm,
                guide,
            });
        }
    };

    // The outlines in reach: the stock's, then each shape's.
    let stock = Polyline::rect(
        Point::new(0.0, 0.0),
        targets.stock.width_mm,
        targets.stock.height_mm,
    );
    let reach = |bounds: Option<Rect>| bounds.is_some_and(|b| near(b, raw, radius_mm));
    let mut outlines: Vec<&Polyline> = Vec::new();
    if reach(stock.bounds()) {
        outlines.push(&stock);
    }
    for shape in targets.scene.shapes() {
        if targets.skip.contains(&shape.instance) || !reach(shape.bounds) {
            continue;
        }
        if let Some(centre) = shape.centre {
            offer(SnapKind::Centre, centre, None);
        }
        for outline in &shape.outlines {
            if !reach(outline.bounds()) {
                continue;
            }
            // A circle that came in as straight segments still has a
            // centre.
            if settings.kinds.contains(&SnapKind::Centre)
                && outline.segments().all(|s| s.is_line())
                && let Some((centre, _)) = as_circle(outline, CIRCLE_TOLERANCE_MM)
            {
                offer(SnapKind::Centre, centre, None);
            }
            outlines.push(outline);
        }
    }
    offer(SnapKind::Endpoint, targets.origin, None);

    // The segments in reach, for the intersections between them.
    let mut near_segments: Vec<(usize, usize, usize, Segment)> = Vec::new();
    for (index, outline) in outlines.iter().enumerate() {
        for vertex in outline.vertices() {
            offer(SnapKind::Endpoint, vertex.point(), None);
        }
        let count = outline.segments().count();
        for (number, segment) in outline.segments().enumerate() {
            if !near(segment_bounds(segment), raw, radius_mm) {
                continue;
            }
            near_segments.push((index, number, count, segment));
            let length_mm = segment.length_mm();
            if let Some((middle, _)) = segment.point_at(length_mm / 2.0) {
                offer(SnapKind::Midpoint, middle, None);
            }
            if let Some(arc) = segment.arc() {
                offer(SnapKind::Centre, arc.centre, None);
            }
            let nearest = closest(segment, raw);
            offer(SnapKind::OnOutline, nearest, Some((raw, nearest)));
            if let Some(from) = reference
                && let Some(foot) = perpendicular_foot(segment, from)
            {
                offer(SnapKind::Perpendicular, foot, Some((from, foot)));
            }
        }
    }
    if settings.kinds.contains(&SnapKind::Intersection) {
        for (i, &(outline_a, number_a, count, a)) in near_segments.iter().enumerate() {
            for &(outline_b, number_b, _, b) in near_segments.iter().skip(i + 1) {
                // Neighbours of one outline share their vertex, which is
                // an endpoint, not a crossing (`shouldCompareProfileSegments`).
                if outline_a == outline_b
                    && adjacent(number_a, number_b, count, outlines.get(outline_a))
                {
                    continue;
                }
                for crossing in crossings(a, b) {
                    offer(SnapKind::Intersection, crossing.at, None);
                }
            }
        }
    }
    drop(offer);

    // The grid: no reach test, so with nothing better the point is on it.
    if settings.kinds.contains(&SnapKind::Grid)
        && let Some(step_mm) = targets.grid_step_mm.filter(|s| s.is_finite() && *s > 0.0)
    {
        let at = match (ray, reference) {
            // Held to a ray, the grid sets the length along it.
            (Some(ray), Some(from)) => {
                let length = (from.distance_to(raw) / step_mm).round() * step_mm;
                from + ray.direction * length
            }
            _ => Point::new(
                (raw.x / step_mm).round() * step_mm,
                (raw.y / step_mm).round() * step_mm,
            ),
        };
        candidates.push(Candidate {
            kind: SnapKind::Grid,
            at,
            distance_mm: raw.distance_to(at),
            guide: None,
        });
    }

    candidates
        .into_iter()
        .min_by(|a, b| {
            (a.kind.rank(), a.distance_mm)
                .partial_cmp(&(b.kind.rank(), b.distance_mm))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map_or(Snapped::free(raw), |best| Snapped {
            at_mm: best.at,
            kind: Some(best.kind),
            guide: best.guide,
        })
}

/// The snap grid's spacing, mm (capability 82a): `pinned_mm` when the
/// user has set one, else the smallest of 1, 2, 5, 10, … millimetres, or
/// of 0.01, 0.02, 0.05, 0.1, … inches for an inch project, whose spacing
/// on screen is at least 8 px. The automatic steps divide the grid lines
/// the Viewport draws (phase 4's 1, 10, 100 mm and 0.1, 1, 10 in), so
/// every drawn line is snapped to.
#[must_use]
pub fn grid_step_mm(px_per_mm: f64, units: Units, pinned_mm: Option<f64>) -> f64 {
    if let Some(pinned) = pinned_mm.filter(|p| p.is_finite() && *p > 0.0) {
        return pinned;
    }
    let unit_mm = match units {
        Units::Mm => 1.0,
        Units::Inch => 25.4,
    };
    if !(px_per_mm.is_finite() && px_per_mm > 0.0) {
        return unit_mm;
    }
    // From a hundredth of a unit up, far past any stock.
    let mut decade = 0.01;
    for _ in 0..8 {
        for factor in [1.0, 2.0, 5.0] {
            let step_mm = decade * factor * unit_mm;
            if step_mm * px_per_mm >= MIN_GRID_SPACING_PX {
                return step_mm;
            }
        }
        decade *= 10.0;
    }
    decade * unit_mm
}

/// `at` held to the nearest direction from `from` that is a multiple of
/// [`ANGLE_STEP_DEG`], its distance from `from` kept: what `Shift` does
/// to a point (Graphite's angle lock). A point on `from` stays there.
#[must_use]
pub fn constrain_angle(from: Point, at: Point) -> Point {
    Ray::through(from, at).map_or(at, |ray| ray.at)
}

/// The ray `Shift` holds a point to.
#[derive(Debug, Clone, Copy)]
struct Ray {
    from: Point,
    direction: Vec2,
    /// The pointer turned onto the ray.
    at: Point,
}

impl Ray {
    /// The ray from `from` nearest the direction of `toward`, at a
    /// multiple of [`ANGLE_STEP_DEG`]. `None` when the two coincide.
    fn through(from: Point, toward: Point) -> Option<Self> {
        let offset = toward - from;
        let length = offset.length();
        if !is_positive(length) {
            return None;
        }
        let step = ANGLE_STEP_DEG.to_radians();
        let angle = (offset.y.atan2(offset.x) / step).round() * step;
        let (sin, cos) = angle.sin_cos();
        // The axes exactly: a horizontal line has no Y in it.
        let snap = |value: f64| if value.abs() < 1e-12 { 0.0 } else { value };
        let direction = Vec2::new(snap(cos), snap(sin));
        Some(Self {
            from,
            direction,
            at: from + direction * length,
        })
    }

    /// How far `p` is from the ray's line, mm; infinite behind its start.
    fn distance_mm(&self, p: Point) -> f64 {
        let offset = p - self.from;
        if offset.dot(self.direction) < 0.0 {
            return f64::INFINITY;
        }
        offset.cross(self.direction).abs()
    }
}

/// Whether `value` is a finite number above zero.
fn is_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// Whether `bounds`, grown by `radius_mm`, holds `p` (`boundsNearPoint`).
fn near(bounds: Rect, p: Point, radius_mm: f64) -> bool {
    p.x >= bounds.min.x - radius_mm
        && p.x <= bounds.max.x + radius_mm
        && p.y >= bounds.min.y - radius_mm
        && p.y <= bounds.max.y + radius_mm
}

/// A box that holds `segment`: its ends for a line, its whole circle for
/// an arc (`segmentBounds`).
fn segment_bounds(segment: Segment) -> Rect {
    match segment.arc() {
        Some(arc) => Rect {
            min: Point::new(arc.centre.x - arc.radius_mm, arc.centre.y - arc.radius_mm),
            max: Point::new(arc.centre.x + arc.radius_mm, arc.centre.y + arc.radius_mm),
        },
        None => Rect::from_point(segment.start).including(segment.end),
    }
}

/// Whether segments `a` and `b` of one outline of `count` segments share
/// a vertex.
fn adjacent(a: usize, b: usize, count: usize, outline: Option<&&Polyline>) -> bool {
    let (low, high) = (a.min(b), a.max(b));
    high - low <= 1 || (outline.is_some_and(|o| o.is_closed()) && low == 0 && high + 1 == count)
}

/// The point of `segment` nearest `p`, following an arc.
fn closest(segment: Segment, p: Point) -> Point {
    let one = Polyline::new(
        vec![
            dc_geometry::Vertex::new(segment.start.x, segment.start.y, segment.bulge),
            dc_geometry::Vertex::line(segment.end.x, segment.end.y),
        ],
        false,
    );
    dc_geometry::nearest_point(&one, p).map_or(segment.start, |nearest| nearest.point)
}

/// The foot of the perpendicular from `from` onto `segment`: the point of
/// the segment where the line to `from` meets it square. `None` when no
/// point of the segment is such a foot (the nearest point is an end), or
/// `from` is on the segment.
fn perpendicular_foot(segment: Segment, from: Point) -> Option<Point> {
    let foot = closest(segment, from);
    let to_from = (from - foot).normalized()?;
    let along = match segment.arc() {
        Some(arc) => (foot - arc.centre).normalized()?.perp(),
        None => (segment.end - segment.start).normalized()?,
    };
    (to_from.dot(along).abs() < 1e-6).then_some(foot)
}
