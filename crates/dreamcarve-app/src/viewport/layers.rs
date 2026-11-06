// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What each Viewport layer holds, as the renderer's segments
//! (`docs/specs/phase-4/design.md`, "The Viewport renderer"; `experience.md`
//! section 9): the grid and the axes over the stock, the stock's outline,
//! the shapes, the hovered shape and the selection (drawn through a
//! drag's preview transform while one is in progress). Arcs are flattened
//! for display at a tolerance of a fraction of a pixel, fixed per zoom
//! band (a doubling of the scale), so a layer is rebuilt when the band
//! changes and not on every zoom step. Display only: nothing here reaches
//! emitted motion.

use dc_app::Scene;
use dc_app::viewport::grid::Grid;
use dc_geometry::{Point, Polyline, Transform};
use dc_project::{InstanceId, Project};

use crate::viewport::renderer::{Segment, StyleId, gpu_f32};

/// The most a flattened arc strays from the true one, screen pixels, at
/// the least zoom of its band; under twice that at the most.
const ARC_TOLERANCE_PX: f64 = 0.25;
/// The finest display tolerance, mm, however far in the camera is.
const MIN_TOLERANCE_MM: f64 = 1e-4;

/// The zoom band of a scale: one per doubling.
#[must_use]
pub(crate) fn band(px_per_mm: f64) -> i32 {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "log2 of a scale between 0.05 and 2000 px/mm"
    )]
    let band = px_per_mm.max(f64::MIN_POSITIVE).log2().floor() as i32;
    band
}

/// The arc-flattening tolerance of zoom band `band`, mm.
#[must_use]
pub(crate) fn tolerance_mm(band: i32) -> f64 {
    (ARC_TOLERANCE_PX / 2f64.powi(band)).max(MIN_TOLERANCE_MM)
}

/// `a`–`b` as a segment starting `along_mm` along its outline.
fn segment(a: Point, b: Point, along_mm: f64, style: StyleId) -> Segment {
    Segment {
        a_mm: [gpu_f32(a.x), gpu_f32(a.y)],
        b_mm: [gpu_f32(b.x), gpu_f32(b.y)],
        along_mm: gpu_f32(along_mm),
        style,
    }
}

/// `points` as segments in order, closing back to the first when `closed`,
/// the dash phase running on along the whole outline.
fn push_path(out: &mut Vec<Segment>, points: &[Point], closed: bool, style: StyleId) {
    let closing = closed
        .then(|| points.last().zip(points.first()))
        .flatten()
        .filter(|_| points.len() > 2);
    let mut along_mm = 0.0;
    let pairs = points
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((*a, *b)),
            _ => None,
        })
        .chain(closing.map(|(a, b)| (*a, *b)));
    for (a, b) in pairs {
        out.push(segment(a, b, along_mm, style));
        along_mm += a.distance_to(b);
    }
}

/// `outline` flattened to `tolerance_mm` and pushed in `style`.
fn push_outline(out: &mut Vec<Segment>, outline: &Polyline, tolerance_mm: f64, style: StyleId) {
    let flat = outline.with_arcs_flattened(tolerance_mm);
    let points: Vec<Point> = flat.vertices().iter().map(|v| v.point()).collect();
    push_path(out, &points, flat.is_closed(), style);
}

/// The grid over the stock (when `grid` is on) and the axes through the
/// origin, clipped to the stock.
#[must_use]
pub(crate) fn grid(project: &Project, grid: Option<Grid>) -> Vec<Segment> {
    let (w, h) = (project.stock.width_mm, project.stock.height_mm);
    let stock = dc_geometry::Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(w, h),
    };
    let mut out = Vec::new();
    if let Some(grid) = grid {
        for line in grid.lines(stock) {
            let style = if line.major {
                StyleId::GridMajor
            } else {
                StyleId::GridMinor
            };
            out.push(segment(line.from, line.to, 0.0, style));
        }
    }
    let origin = project.origin.xy.point_mm(&project.stock);
    if (0.0..=w).contains(&origin.x) {
        out.push(segment(
            Point::new(origin.x, 0.0),
            Point::new(origin.x, h),
            0.0,
            StyleId::Axis,
        ));
    }
    if (0.0..=h).contains(&origin.y) {
        out.push(segment(
            Point::new(0.0, origin.y),
            Point::new(w, origin.y),
            0.0,
            StyleId::Axis,
        ));
    }
    out
}

/// The stock's dashed outline.
#[must_use]
pub(crate) fn stock(project: &Project) -> Vec<Segment> {
    let (w, h) = (project.stock.width_mm, project.stock.height_mm);
    let mut out = Vec::with_capacity(4);
    push_path(
        &mut out,
        &[
            Point::new(0.0, 0.0),
            Point::new(w, 0.0),
            Point::new(w, h),
            Point::new(0.0, h),
        ],
        true,
        StyleId::Stock,
    );
    out
}

/// `outline` flattened to `tolerance_mm`, then placed by `placed` (a drag's
/// preview), pushed in `style`. Flattening first keeps an arc under an
/// uneven scale drawable: display only, the command checks the arcs.
fn push_placed(
    out: &mut Vec<Segment>,
    outline: &Polyline,
    tolerance_mm: f64,
    placed: Transform,
    style: StyleId,
) {
    let flat = outline.with_arcs_flattened(tolerance_mm);
    let points: Vec<Point> = flat
        .vertices()
        .iter()
        .map(|v| placed.apply(v.point()))
        .collect();
    push_path(out, &points, flat.is_closed(), style);
}

/// Every visible shape but those in `moving` (drawn by the selection layer
/// where a drag will put them): closed solid, open dashed, locked in its
/// grey.
#[must_use]
pub(crate) fn shapes(scene: &Scene, tolerance_mm: f64, moving: &[InstanceId]) -> Vec<Segment> {
    let mut out = Vec::new();
    for shape in scene
        .shapes()
        .iter()
        .filter(|s| !moving.contains(&s.instance))
    {
        for outline in &shape.outlines {
            let style = match (shape.locked, outline.is_closed()) {
                (false, true) => StyleId::Shape,
                (false, false) => StyleId::ShapeOpen,
                (true, true) => StyleId::Locked,
                (true, false) => StyleId::LockedOpen,
            };
            push_outline(&mut out, outline, tolerance_mm, style);
        }
    }
    out
}

/// The shapes among `ids`, in `closed` / `open` styles; those among
/// `preview`'s ids placed by its transform.
#[must_use]
pub(crate) fn highlighted(
    scene: &Scene,
    ids: &[InstanceId],
    tolerance_mm: f64,
    (closed, open): (StyleId, StyleId),
    preview: Option<(&[InstanceId], Transform)>,
) -> Vec<Segment> {
    let mut out = Vec::new();
    for id in ids {
        let Some(shape) = scene.shape(id) else {
            continue;
        };
        let placed = preview
            .filter(|(moving, _)| moving.contains(id))
            .map(|(_, t)| t);
        for outline in &shape.outlines {
            let style = if outline.is_closed() { closed } else { open };
            match placed {
                Some(t) => push_placed(&mut out, outline, tolerance_mm, t, style),
                None => push_outline(&mut out, outline, tolerance_mm, style),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One band per doubling; a band's tolerance keeps a flattened arc
    /// within half a pixel at every zoom in it.
    #[test]
    fn arcs_flatten_under_half_a_pixel_in_every_band() {
        for px_per_mm in [0.05, 0.3, 1.0, 1.99, 2.0, 7.5, 150.0, 2000.0] {
            let b = band(px_per_mm);
            assert!(
                2f64.powi(b) <= px_per_mm && px_per_mm < 2f64.powi(b + 1),
                "{px_per_mm} px/mm in band {b}"
            );
            let error_px = tolerance_mm(b) * px_per_mm;
            assert!(error_px < 0.5 + 1e-9, "{px_per_mm} px/mm: {error_px} px");
        }
    }

    /// A closed path closes once, and the dash phase runs on along it.
    #[test]
    #[allow(clippy::float_cmp, reason = "whole millimetres, exact in f32")]
    fn paths_close_and_carry_their_length() {
        let mut out = Vec::new();
        let square = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        push_path(&mut out, &square, true, StyleId::Shape);
        let along: Vec<f32> = out.iter().map(|s| s.along_mm).collect();
        assert_eq!(along, [0.0, 10.0, 20.0, 30.0]);
        assert_eq!(out.last().map(|s| s.b_mm), Some([0.0, 0.0]));
        out.clear();
        push_path(&mut out, &square, false, StyleId::ShapeOpen);
        assert_eq!(out.len(), 3);
    }
}
