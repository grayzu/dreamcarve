// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/canvas/hitTest.ts

//! Picking on the Viewport (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app", `pick`; functional spec, capability 75): what a click at a
//! pane point selects, from the [`Scene`]'s project-space outlines.
//!
//! From PureCutCNC's `hitTest.ts`: a shape is a candidate when the point
//! is within the pixel tolerance of one of its outlines or inside one of
//! its closed outlines; a unique nearby outline wins over interiors; among
//! interiors the innermost (enclosed by the most others) wins, then the
//! topmost. Upstream returns *ambiguous* when two or more outlines are
//! near and shows a picker; DreamCarve orders those candidates nearest
//! first (topmost on a tie) and a second click on the same spot takes the
//! next one ([`ClickCycle`]), as capability 75 asks. Only what the Scene
//! holds is picked, so a hidden shape never is and a locked one is.
//!
//! The male inlay board's shapes (slice 14, #543) are the female's
//! instances seen from the other board, so they are never candidates: a
//! click that finds no instance and lands on one, by the same tolerance
//! and containment rules, is [`Hit::MaleBoardShape`] of its Inlay.
//!
//! Re-derived for Y-up: nothing here depends on direction. Distances and
//! containment are taken in project millimetres, with the tolerance
//! converted at the camera's scale, as upstream does; arcs are measured as
//! arcs (`dc_geometry::point_segment_distance_mm`), and containment is
//! even-odd over each outline flattened at [`crate::scene::PICK_FLATTEN_MM`].

use dc_geometry::{Point, Polyline, Rect, point_segment_distance_mm};
use dc_project::{InstanceId, OperationId};

use crate::scene::{Scene, Shape};
use crate::viewport::camera::{Camera, Pos};

/// How far from an outline a click still hits it, logical pixels
/// (PureCutCNC's `pointNearProfile` default).
pub const PICK_TOLERANCE_PX: f64 = 8.0;
/// Two outlines whose distances from the click differ by no more than
/// this are a tie, logical pixels, and the topmost goes first.
const TIE_PX: f64 = 0.5;
/// How far a click may land from the last one and still be "the same
/// place" for [`ClickCycle`], logical pixels.
const SAME_PLACE_PX: f64 = 3.0;

/// What a click on the Viewport lands on. Tabs and toolpaths join with
/// the slices that draw them (17, 16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// A feature instance.
    Instance(InstanceId),
    /// A shape on the male inlay board (slice 14, #543): it selects its
    /// Inlay.
    MaleBoardShape(OperationId),
    /// Empty stock or empty stage.
    Nothing,
}

/// One candidate under the pointer.
struct Candidate<'s> {
    shape: &'s Shape,
    /// Draw order: higher is on top.
    order: usize,
    /// Distance to the nearest outline, mm, when within the tolerance.
    near_mm: Option<f64>,
}

/// Whether `p` is inside the closed outlines of `flat` by the even-odd
/// rule (PureCutCNC `pointInProfile`, over every closed profile of the
/// shape, so a letter's counter is outside it).
fn inside(flat: &[Polyline], p: Point) -> bool {
    let mut inside = false;
    for outline in flat.iter().filter(|o| o.is_closed()) {
        let vertices = outline.vertices();
        let Some(last) = vertices.last() else {
            continue;
        };
        let mut j = last.point();
        for v in vertices {
            let i = v.point();
            if (i.y > p.y) != (j.y > p.y) && p.x < (j.x - i.x) * (p.y - i.y) / (j.y - i.y) + i.x {
                inside = !inside;
            }
            j = i;
        }
    }
    inside
}

/// The distance from `p` to the nearest of `outlines`, mm.
fn distance_mm(outlines: &[Polyline], p: Point) -> Option<f64> {
    outlines
        .iter()
        .flat_map(Polyline::segments)
        .map(|seg| point_segment_distance_mm(seg, p))
        .reduce(f64::min)
}

/// Whether every vertex of `inner`'s closed outlines is inside `outer`'s
/// (PureCutCNC `profileContainedIn`): `inner` is enclosed by `outer`
/// whatever the draw order. Coincident shapes enclose each other, a tie.
fn enclosed_by(inner: &Shape, outer: &Shape) -> bool {
    let mut any = false;
    for outline in inner.flat.iter().filter(|o| o.is_closed()) {
        for v in outline.vertices() {
            any = true;
            if !inside(&outer.flat, v.point()) {
                return false;
            }
        }
    }
    any
}

/// Every shape under pane point `at`, in the order clicks cycle through
/// them: shapes with an outline within `tolerance_px`, nearest first;
/// then shapes whose inside holds the point, innermost first; topmost
/// first on a tie.
#[must_use]
pub fn candidates(scene: &Scene, camera: &Camera, at: Pos, tolerance_px: f64) -> Vec<InstanceId> {
    let p = camera.to_world(at);
    let tolerance_mm = camera.px_to_mm(tolerance_px);
    let tie_mm = camera.px_to_mm(TIE_PX);
    let mut found: Vec<Candidate<'_>> = Vec::new();
    for (order, shape) in scene.shapes().iter().enumerate() {
        let Some(bounds) = shape.bounds else {
            continue;
        };
        let reach = Rect {
            min: Point::new(bounds.min.x - tolerance_mm, bounds.min.y - tolerance_mm),
            max: Point::new(bounds.max.x + tolerance_mm, bounds.max.y + tolerance_mm),
        };
        if p.x < reach.min.x || p.x > reach.max.x || p.y < reach.min.y || p.y > reach.max.y {
            continue;
        }
        let near_mm = distance_mm(&shape.outlines, p).filter(|d| *d <= tolerance_mm);
        if near_mm.is_some() || inside(&shape.flat, p) {
            found.push(Candidate {
                shape,
                order,
                near_mm,
            });
        }
    }

    let (mut near, interior): (Vec<_>, Vec<_>) =
        found.into_iter().partition(|c| c.near_mm.is_some());
    // Nearest outline first; within a tie, the topmost.
    near.sort_by(|a, b| {
        let (da, db) = (a.near_mm.unwrap_or(0.0), b.near_mm.unwrap_or(0.0));
        if (da - db).abs() <= tie_mm {
            b.order.cmp(&a.order)
        } else {
            da.total_cmp(&db)
        }
    });
    // Innermost first: enclosed by the most other candidates.
    let enclosing: Vec<usize> = interior
        .iter()
        .map(|c| {
            interior
                .iter()
                .filter(|o| o.order != c.order && enclosed_by(c.shape, o.shape))
                .count()
        })
        .collect();
    let mut ranked: Vec<(usize, Candidate<'_>)> = enclosing.into_iter().zip(interior).collect();
    ranked.sort_by(|(ea, a), (eb, b)| eb.cmp(ea).then(b.order.cmp(&a.order)));

    near.into_iter()
        .chain(ranked.into_iter().map(|(_, c)| c))
        .map(|c| c.shape.instance.clone())
        .collect()
}

/// The Inlay whose shape on the male board is under pane point `at`: an
/// outline within `tolerance_px`, else the inside of its closed outlines;
/// the topmost (the last) where two are.
#[must_use]
pub fn male_board_shape(
    scene: &Scene,
    camera: &Camera,
    at: Pos,
    tolerance_px: f64,
) -> Option<OperationId> {
    let board = scene.male_board()?;
    let p = camera.to_world(at);
    let tolerance_mm = camera.px_to_mm(tolerance_px);
    let near = |flat: &[Polyline], outlines: &[Polyline]| {
        distance_mm(outlines, p).is_some_and(|d| d <= tolerance_mm) || inside(flat, p)
    };
    board
        .shapes
        .iter()
        .rev()
        .find(|shape| near(&shape.flat, &shape.outlines))
        .map(|shape| shape.op.clone())
}

/// What a click at pane point `at` picks: the `cycle`-th of
/// [`candidates`], wrapping; with none, the male board's shape there
/// ([`male_board_shape`]); else [`Hit::Nothing`].
#[must_use]
pub fn pick(scene: &Scene, camera: &Camera, at: Pos, tolerance_px: f64, cycle: usize) -> Hit {
    let found = candidates(scene, camera, at, tolerance_px);
    match found.len() {
        0 => male_board_shape(scene, camera, at, tolerance_px)
            .map_or(Hit::Nothing, Hit::MaleBoardShape),
        n => found
            .into_iter()
            .nth(cycle % n)
            .map_or(Hit::Nothing, Hit::Instance),
    }
}

/// Whether segment `a`–`b` meets `rect` (Liang–Barsky clipping).
fn segment_meets(a: Point, b: Point, rect: Rect) -> bool {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let mut t0: f64 = 0.0;
    let mut t1: f64 = 1.0;
    for (p, q) in [
        (-dx, a.x - rect.min.x),
        (dx, rect.max.x - a.x),
        (-dy, a.y - rect.min.y),
        (dy, rect.max.y - a.y),
    ] {
        if p.abs() <= f64::EPSILON {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// The straight edges of `outline` as drawn, a lone vertex as a point.
fn edges(outline: &Polyline) -> Vec<(Point, Point)> {
    let points: Vec<Point> = outline.vertices().iter().map(|v| v.point()).collect();
    let mut edges: Vec<(Point, Point)> = points
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        })
        .collect();
    match points.as_slice() {
        [only] => edges.push((*only, *only)),
        [first, .., last] if outline.is_closed() => edges.push((*last, *first)),
        _ => {}
    }
    edges
}

/// What a marquee over `rect` (mm) selects (`experience.md`, section 8: "a
/// marquee on empty stock selects what it touches"): every shape an
/// outline of which lies in or crosses the rectangle, in draw order.
#[must_use]
pub fn touching(scene: &Scene, rect: Rect) -> Vec<InstanceId> {
    let overlaps = |b: Rect| {
        b.max.x >= rect.min.x
            && b.min.x <= rect.max.x
            && b.max.y >= rect.min.y
            && b.min.y <= rect.max.y
    };
    scene
        .shapes()
        .iter()
        .filter(|shape| {
            shape.bounds.is_some_and(overlaps)
                && shape
                    .flat
                    .iter()
                    .flat_map(edges)
                    .any(|(a, b)| segment_meets(a, b, rect))
        })
        .map(|shape| shape.instance.clone())
        .collect()
}

/// Repeated clicks in one place (capability 75: "a second click cycles"):
/// a click within a few pixels of the last one, over the same candidates,
/// takes the next candidate down; anywhere else starts again at the first.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClickCycle {
    last: Option<(Pos, Vec<InstanceId>, usize)>,
}

impl ClickCycle {
    /// What a click at pane point `at` picks, remembering it for the next.
    pub fn click(&mut self, scene: &Scene, camera: &Camera, at: Pos, tolerance_px: f64) -> Hit {
        let found = candidates(scene, camera, at, tolerance_px);
        let index = match &self.last {
            Some((pos, ids, index)) if pos.distance_to(at) <= SAME_PLACE_PX && *ids == found => {
                index + 1
            }
            _ => 0,
        };
        let hit = match found.len() {
            0 => male_board_shape(scene, camera, at, tolerance_px)
                .map_or(Hit::Nothing, Hit::MaleBoardShape),
            n => found
                .get(index % n)
                .cloned()
                .map_or(Hit::Nothing, Hit::Instance),
        };
        self.last = Some((at, found, index));
        hit
    }

    /// Forgets the last click, so the next starts at the first candidate.
    pub fn reset(&mut self) {
        self.last = None;
    }
}
