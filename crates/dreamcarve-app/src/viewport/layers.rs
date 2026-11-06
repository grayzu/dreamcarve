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
use dc_app::scene::{SceneOperation, StrokeKind, strokes};
use dc_app::view::{DepthRamp, Visibility};
use dc_app::viewport::arrows::{ArrowCache, ArrowMove, ArrowVisibility, moves_of};
use dc_app::viewport::grid::Grid;
use dc_geometry::{Point, Point3, Polyline, Rect, Transform};
use dc_project::{Board, InstanceId, Project};

use crate::viewport::lod;
use crate::viewport::renderer::{Segment, StyleId, StyleWord, gpu_f32};

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
fn segment(a: Point, b: Point, along_mm: f64, style: impl Into<StyleWord>) -> Segment {
    Segment {
        a_mm: [gpu_f32(a.x), gpu_f32(a.y)],
        b_mm: [gpu_f32(b.x), gpu_f32(b.y)],
        along_mm: gpu_f32(along_mm),
        style: style.into(),
    }
}

/// A ring of `radius_mm` about `centre` (never under its style's least
/// radius in pixels).
fn ring(centre: Point, radius_mm: f64, style: StyleWord) -> Segment {
    segment(centre, centre, radius_mm, style)
}

/// `points` as segments in order, closing back to the first when `closed`,
/// the dash phase running on along the whole outline.
fn push_path(out: &mut Vec<Segment>, points: &[Point], closed: bool, style: impl Into<StyleWord>) {
    let style = style.into();
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

/// The coalescing length of zoom band `band`, mm: one pixel at the band's
/// top scale, so a coalesced run never covers more than a pixel there and
/// half of one at the band's bottom.
#[must_use]
pub(crate) fn lod_mm(band: i32) -> f64 {
    1.0 / 2f64.powi(band.saturating_add(1))
}

/// The XY of `p`.
const fn xy(p: Point3) -> Point {
    Point::new(p.x, p.y)
}

/// A move with no more XY travel than this draws no line, mm.
const XY_EPS_MM: f64 = 1e-9;

/// How one operation's toolpath is drawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ToolpathLook<'a> {
    /// Which layers are on.
    pub visibility: &'a Visibility,
    /// The project's depth ramp, so cutting moves carry their step.
    pub ramp: Option<&'a DepthRamp>,
    /// The arc-flattening tolerance of the zoom band, mm.
    pub tolerance_mm: f64,
    /// The coalescing length of the zoom band, mm.
    pub lod_mm: f64,
}

/// Operation `op`'s toolpath buffer: its runs' rapids, then cuts (each in
/// the run's preview colour), then tab marks, plunge marks, and start
/// points with their ticks or a Drill's holes, as `look` asks; everything
/// faded while `op` is stale. Male board runs are left for slice 14.
#[must_use]
pub(crate) fn toolpath(op: &SceneOperation, look: ToolpathLook<'_>) -> Vec<Segment> {
    let visibility = look.visibility;
    let stale = op.stale;
    let mut rapids = Vec::new();
    let mut cuts = Vec::new();
    let mut marks = Vec::new();
    for (k, run) in op.runs.iter().enumerate() {
        if run.run.board == Board::Male {
            continue;
        }
        let colour = Some(op.colour + k);
        let mut last_plunge: Option<Point> = None;
        for stroke in strokes(&run.run.toolpath) {
            match stroke.kind {
                StrokeKind::Rapid if visibility.rapids => {
                    if xy(stroke.from).distance_to(xy(stroke.to)) > XY_EPS_MM {
                        let word = StyleWord::toolpath(StyleId::Rapid, None, None, stale);
                        rapids.push(segment(xy(stroke.from), xy(stroke.to), 0.0, word));
                    }
                }
                StrokeKind::Cut if visibility.cuts => {
                    for pair in stroke.points(look.tolerance_mm).windows(2) {
                        let [a, b] = pair else { continue };
                        if xy(*a).distance_to(xy(*b)) <= XY_EPS_MM {
                            continue;
                        }
                        let depth = look.ramp.map(|r| r.step(a.z.min(b.z)));
                        let word = StyleWord::toolpath(StyleId::Cut, colour, depth, stale);
                        cuts.push(segment(xy(*a), xy(*b), 0.0, word));
                    }
                }
                StrokeKind::Plunge if visibility.plunges => {
                    let at = xy(stroke.to);
                    if last_plunge.is_none_or(|p| p.distance_to(at) > XY_EPS_MM) {
                        let word = StyleWord::toolpath(StyleId::Plunge, None, None, stale);
                        marks.push(ring(at, 0.0, word));
                    }
                    last_plunge = Some(at);
                }
                _ => {}
            }
        }
        if visibility.tab_marks {
            let word = StyleWord::toolpath(StyleId::Tab, None, None, stale);
            for tab in &run.marks.tab_marks {
                push_outline_word(&mut marks, tab, look.tolerance_mm, word);
            }
        }
        if visibility.start_points {
            let circle = StyleWord::toolpath(StyleId::Mark, colour, None, stale);
            let tick = StyleWord::toolpath(StyleId::Tick, colour, None, stale);
            for start in &run.marks.start_points {
                marks.push(ring(start.at, start.radius_mm, circle));
                if let Some(direction) = start.direction {
                    let end = start.at + direction * (2.0 * start.radius_mm);
                    marks.push(segment(start.at, end, 0.0, tick));
                }
            }
            for (hole, radius_mm) in &run.marks.holes {
                marks.push(ring(*hole, *radius_mm, circle));
            }
        }
    }
    let mut out = lod::coalesce(&rapids, look.lod_mm);
    out.extend(lod::coalesce(&cuts, look.lod_mm));
    out.extend(marks);
    out
}

/// `outline` flattened to `tolerance_mm` and pushed with `word`.
fn push_outline_word(
    out: &mut Vec<Segment>,
    outline: &Polyline,
    tolerance_mm: f64,
    word: StyleWord,
) {
    let flat = outline.with_arcs_flattened(tolerance_mm);
    let points: Vec<Point> = flat.vertices().iter().map(|v| v.point()).collect();
    push_path(out, &points, flat.is_closed(), word);
}

/// A direction arrow's arm, logical pixels (`experience.md`, section 9:
/// 6-px chevrons).
const ARROW_ARM_PX: f64 = 6.0;
/// The angle between a chevron's arm and its move, radians.
const ARROW_HALF_ANGLE_RAD: f64 = 0.6;

/// One drawn run's arrows: its cache key, its moves as the pass reads
/// them, its colour and whether it is stale.
pub(crate) struct ArrowRun<'a> {
    pub key: u64,
    pub moves: &'a [ArrowMove],
    pub colour: usize,
    pub stale: bool,
}

/// The direction arrows of `runs` at `px_per_mm`: a chevron at the middle
/// of each move [`dc_app::viewport::arrows`] places an arrow on, pointing
/// the way the cut goes, in the run's colour. Cuts only (capability 72:
/// "Direction arrows along cuts").
#[must_use]
pub(crate) fn arrows(
    runs: &[ArrowRun<'_>],
    cache: &mut ArrowCache,
    px_per_mm: f64,
    visibility: &Visibility,
) -> Vec<Segment> {
    let mut out = Vec::new();
    if px_per_mm <= 0.0 {
        return out;
    }
    let gate = ArrowVisibility {
        cuts: visibility.cuts,
        rapids: false,
    };
    let arm_mm = ARROW_ARM_PX / px_per_mm;
    let (sin, cos) = ARROW_HALF_ANGLE_RAD.sin_cos();
    for run in runs {
        let word = StyleWord::toolpath(StyleId::Tick, Some(run.colour), None, run.stale);
        for arrow in &cache.placements(run.key, run.moves, px_per_mm, gate).cut {
            let [fx, fy, tx, ty] = *arrow;
            let (dx, dy) = (tx - fx, ty - fy);
            let length = dx.hypot(dy);
            if length <= 0.0 {
                continue;
            }
            let (ux, uy) = (dx / length, dy / length);
            let mid = Point::new((fx + tx) / 2.0 / px_per_mm, (fy + ty) / 2.0 / px_per_mm);
            let tip = Point::new(mid.x + ux * arm_mm / 2.0, mid.y + uy * arm_mm / 2.0);
            for side in [1.0, -1.0] {
                // The move's direction turned by ± the half angle, back
                // from the tip.
                let (bx, by) = (ux * cos - side * uy * sin, uy * cos + side * ux * sin);
                let end = Point::new(tip.x - bx * arm_mm, tip.y - by * arm_mm);
                out.push(segment(tip, end, 0.0, word));
            }
        }
    }
    out
}

/// `toolpath`'s moves for the arrow pass at `tolerance_mm`.
#[must_use]
pub(crate) fn arrow_moves(toolpath: &dc_job::Toolpath, tolerance_mm: f64) -> Vec<ArrowMove> {
    moves_of(toolpath, tolerance_mm)
}

/// The program extents (capability 74): the tool centre's box solid and,
/// outside it, the bit edge's dashed.
#[must_use]
pub(crate) fn extents(centre: Rect, edge: Rect) -> Vec<Segment> {
    let corners = |r: Rect| {
        [
            r.min,
            Point::new(r.max.x, r.min.y),
            r.max,
            Point::new(r.min.x, r.max.y),
        ]
    };
    let mut out = Vec::with_capacity(8);
    push_path(&mut out, &corners(centre), true, StyleId::ExtentsCentre);
    push_path(&mut out, &corners(edge), true, StyleId::ExtentsEdge);
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
