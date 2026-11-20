// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What each Viewport layer holds, as the renderer's segments
//! (`docs/specs/phase-4/design.md`, "The Viewport renderer"; `experience.md`
//! section 9): the grid and the axes over the stock, the stock's outline,
//! the shapes, the hovered shape and the selection (drawn through a
//! drag's preview transform while one is in progress). While an
//! operation's targets are marked (slice 27, #619; capability 67) the
//! shapes that are not its targets fade to 40 % and its targets draw over
//! the toolpaths, 2 px in its colour ([`targets`]). With an Inlay, the
//! stock layer carries the male board beside it too (slice 14, #543): its
//! dashed outline and the design mirrored on it, and its male runs are
//! drawn moved onto it by the Scene's display offset. Arcs are flattened
//! for display at a tolerance of a fraction of a pixel, fixed per zoom
//! band (a doubling of the scale), so a layer is rebuilt when the band
//! changes and not on every zoom step. Display only: nothing here reaches
//! emitted motion.

use dc_app::Scene;
use dc_app::scene::{SceneOperation, SceneRun, ShapeStyle, StrokeKind, strokes};
use dc_app::view::{DepthRamp, Visibility};
use dc_app::viewport::arrows::{ArrowCache, ArrowMove, ArrowVisibility, moves_of};
use dc_app::viewport::grid::Grid;
use dc_geometry::{Point, Point3, Polyline, Rect, Transform, Vec2};
use dc_project::{Board, InstanceId, Project};

use crate::viewport::lod;
use crate::viewport::renderer::{Segment, StyleId, StyleWord, gpu_f32};

/// The most a flattened arc strays from the true one, in points, at the
/// least zoom of its band; under twice that at the most. An eighth: at
/// the top of a band on a 2× display that is half a device pixel, under
/// what a 1.25-px anti-aliased line shows, so a circle reads as a circle.
/// At a quarter the coaster's outline showed its chords there.
const ARC_TOLERANCE_PX: f64 = 0.125;
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
fn push_outline(
    out: &mut Vec<Segment>,
    outline: &Polyline,
    tolerance_mm: f64,
    style: impl Into<StyleWord>,
) {
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

/// The stock's dashed outline and, with an Inlay, the male board's
/// beside it (`viewport.male-board`, dashed 6/4) with the design mirrored
/// on it and, when the Inlay has one, its drawn boundary in the board's
/// own dashes, their arcs flattened to `tolerance_mm`.
#[must_use]
pub(crate) fn stock(project: &Project, scene: &Scene, tolerance_mm: f64) -> Vec<Segment> {
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
    if let Some(board) = scene.male_board() {
        let (min, max) = (board.frame.min, board.frame.max);
        push_path(
            &mut out,
            &[min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)],
            true,
            StyleId::MaleBoard,
        );
        for shape in &board.shapes {
            for outline in &shape.outlines {
                push_outline(&mut out, outline, tolerance_mm, StyleId::MaleShape);
            }
        }
        // A drawn boundary is the band's outer edge: dashed in the
        // board's grey, as `--preview` draws it.
        for outline in &board.boundaries {
            push_outline(&mut out, outline, tolerance_mm, StyleId::MaleBoard);
        }
    }
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
/// grey; while an operation's targets are marked, every shape that is not
/// one of them at 40 % (capability 67).
#[must_use]
pub(crate) fn shapes(scene: &Scene, tolerance_mm: f64, moving: &[InstanceId]) -> Vec<Segment> {
    let mut out = Vec::new();
    for shape in scene
        .shapes()
        .iter()
        .filter(|s| !moving.contains(&s.instance))
    {
        let faded = scene.style(&shape.instance) == ShapeStyle::Faded;
        for outline in &shape.outlines {
            let style = match (shape.locked, outline.is_closed()) {
                (false, true) => StyleId::Shape,
                (false, false) => StyleId::ShapeOpen,
                (true, true) => StyleId::Locked,
                (true, false) => StyleId::LockedOpen,
            };
            let word = if faded {
                StyleWord::faded(style)
            } else {
                StyleWord::from(style)
            };
            push_outline(&mut out, outline, tolerance_mm, word);
        }
    }
    out
}

/// The marked operation's targets (capability 67; `experience.md`,
/// section 9): each visible target's outlines 2 px in the operation's
/// colour, drawn over the toolpaths; one that is also among `selected`
/// 4 px, a halo under its selection stroke. Those in `moving` are left
/// to the selection layer, which draws them where a drag will put them.
#[must_use]
pub(crate) fn targets(
    scene: &Scene,
    tolerance_mm: f64,
    selected: &[InstanceId],
    moving: &[InstanceId],
) -> Vec<Segment> {
    let mut out = Vec::new();
    let Some(marks) = scene.targets() else {
        return out;
    };
    for id in marks.targets.iter().filter(|id| !moving.contains(id)) {
        let Some(shape) = scene.shape(id) else {
            continue;
        };
        let halo = selected.contains(id);
        for outline in &shape.outlines {
            let style = match (halo, outline.is_closed()) {
                (false, true) => StyleId::Target,
                (false, false) => StyleId::TargetOpen,
                (true, true) => StyleId::TargetHalo,
                (true, false) => StyleId::TargetHaloOpen,
            };
            push_outline(
                &mut out,
                outline,
                tolerance_mm,
                StyleWord::target(style, marks.colour),
            );
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
    /// Where a male board run is drawn relative to its board's space
    /// ([`Scene::board_offset`]); `None` draws none.
    pub male_offset: Option<Vec2>,
}

/// Operation `op`'s toolpath buffer: its runs' rapids, then cuts, then
/// tab marks, plunge marks, and start points with their ticks or a
/// Drill's holes, as `look` asks; everything faded while `op` is stale.
/// Every run's cuts and marks are in the operation's one colour
/// (`dc_app::operations::colour_at`, its row's; capability 73), a
/// clearing pass's lighter (capability 70). A male board run is drawn on
/// the male board, moved by `look.male_offset` (display only).
#[must_use]
pub(crate) fn toolpath(op: &SceneOperation, look: ToolpathLook<'_>) -> Vec<Segment> {
    let visibility = look.visibility;
    let stale = op.stale;
    let mut rapids = Vec::new();
    let mut cuts = Vec::new();
    let mut marks = Vec::new();
    for run in op.runs.iter() {
        let offset = match run.run.board {
            Board::Female => Vec2::new(0.0, 0.0),
            Board::Male => match look.male_offset {
                Some(offset) => offset,
                None => continue,
            },
        };
        let on = |p: Point| p + offset;
        let tint = Tint::of(op, run);
        let mut last_plunge: Option<Point> = None;
        for stroke in strokes(&run.run.toolpath) {
            match stroke.kind {
                StrokeKind::Rapid if visibility.rapids => {
                    if xy(stroke.from).distance_to(xy(stroke.to)) > XY_EPS_MM {
                        let word = StyleWord::toolpath(StyleId::Rapid, None, None, stale);
                        rapids.push(segment(on(xy(stroke.from)), on(xy(stroke.to)), 0.0, word));
                    }
                }
                StrokeKind::Cut if visibility.cuts => {
                    let mut push = |a: Point3, b: Point3| {
                        if xy(a).distance_to(xy(b)) > XY_EPS_MM {
                            let depth = look.ramp.map(|r| r.step(a.z.min(b.z)));
                            let word = tint.word(StyleId::Cut, depth);
                            cuts.push(segment(on(xy(a)), on(xy(b)), 0.0, word));
                        }
                    };
                    // A line needs no flattening, and most moves are lines.
                    if stroke.arc.is_none() {
                        push(stroke.from, stroke.to);
                    } else {
                        for pair in stroke.points(look.tolerance_mm).windows(2) {
                            if let [a, b] = pair {
                                push(*a, *b);
                            }
                        }
                    }
                }
                StrokeKind::Plunge if visibility.plunges => {
                    let at = xy(stroke.to);
                    if last_plunge.is_none_or(|p| p.distance_to(at) > XY_EPS_MM) {
                        let word = StyleWord::toolpath(StyleId::Plunge, None, None, stale);
                        marks.push(ring(on(at), 0.0, word));
                    }
                    last_plunge = Some(at);
                }
                _ => {}
            }
        }
        if visibility.tab_marks {
            let word = StyleWord::toolpath(StyleId::Tab, None, None, stale);
            for tab in &run.marks.tab_marks {
                push_outline_word(&mut marks, tab, look.tolerance_mm, offset, word);
            }
        }
        if visibility.start_points {
            let circle = tint.word(StyleId::Mark, None);
            let tick = tint.word(StyleId::Tick, None);
            for start in &run.marks.start_points {
                marks.push(ring(on(start.at), start.radius_mm, circle));
                if let Some(direction) = start.direction {
                    let end = start.at + direction * (2.0 * start.radius_mm);
                    marks.push(segment(on(start.at), on(end), 0.0, tick));
                }
            }
            for (hole, radius_mm) in &run.marks.holes {
                marks.push(ring(on(*hole), *radius_mm, circle));
            }
        }
    }
    let mut out = lod::coalesce(&rapids, look.lod_mm);
    out.extend(lod::coalesce(&cuts, look.lod_mm));
    out.extend(marks);
    out
}

/// `outline` flattened to `tolerance_mm`, moved by `offset`, and pushed
/// with `word`.
fn push_outline_word(
    out: &mut Vec<Segment>,
    outline: &Polyline,
    tolerance_mm: f64,
    offset: Vec2,
    word: StyleWord,
) {
    let flat = outline.with_arcs_flattened(tolerance_mm);
    let points: Vec<Point> = flat.vertices().iter().map(|v| v.point() + offset).collect();
    push_path(out, &points, flat.is_closed(), word);
}

/// A direction arrow's arm, logical pixels (`experience.md`, section 9:
/// 6-px chevrons).
const ARROW_ARM_PX: f64 = 6.0;
/// The angle between a chevron's arm and its move, radians.
const ARROW_HALF_ANGLE_RAD: f64 = 0.6;

/// How a run's coloured segments draw: in its operation's colour
/// (`dc_app::operations::colour_at`), faded while the operation is stale,
/// lighter when the run is a clearing pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Tint {
    pub colour: usize,
    pub stale: bool,
    pub lighter: bool,
}

impl Tint {
    /// The tint of `run` of `op`.
    pub(crate) fn of(op: &SceneOperation, run: &SceneRun) -> Self {
        Self {
            colour: op.colour,
            stale: op.stale,
            lighter: run.is_clearing(),
        }
    }

    /// A segment of row `style` in the tint, at depth step `depth` when
    /// it cuts.
    fn word(self, style: StyleId, depth: Option<u8>) -> StyleWord {
        StyleWord::toolpath(style, Some(self.colour), depth, self.stale).lighter(self.lighter)
    }
}

/// One drawn run's arrows: its cache key, its moves as the pass reads
/// them, its tint, and where it is drawn relative to its board's space (a
/// male board run's display offset).
pub(crate) struct ArrowRun<'a> {
    pub key: u64,
    pub moves: &'a [ArrowMove],
    pub tint: Tint,
    pub offset: Vec2,
}

/// The direction arrows of `runs` at `px_per_mm`: a chevron at the middle
/// of each move [`dc_app::viewport::arrows`] places an arrow on, pointing
/// the way the cut goes, in its operation's colour. Cuts only (capability 72:
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
        let word = run.tint.word(StyleId::Tick, None);
        for arrow in &cache.placements(run.key, run.moves, px_per_mm, gate).cut {
            let [fx, fy, tx, ty] = *arrow;
            let (dx, dy) = (tx - fx, ty - fy);
            let length = dx.hypot(dy);
            if length <= 0.0 {
                continue;
            }
            let (ux, uy) = (dx / length, dy / length);
            let mid =
                Point::new((fx + tx) / 2.0 / px_per_mm, (fy + ty) / 2.0 / px_per_mm) + run.offset;
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
    /// within a quarter of a point at every zoom in it, which is half a
    /// device pixel on a 2× display.
    #[test]
    fn arcs_flatten_under_a_quarter_point_in_every_band() {
        for px_per_mm in [0.05, 0.3, 1.0, 1.99, 2.0, 7.5, 150.0, 2000.0] {
            let b = band(px_per_mm);
            assert!(
                2f64.powi(b) <= px_per_mm && px_per_mm < 2f64.powi(b + 1),
                "{px_per_mm} px/mm in band {b}"
            );
            let error_px = tolerance_mm(b) * px_per_mm;
            assert!(error_px < 0.25 + 1e-9, "{px_per_mm} px/mm: {error_px} px");
        }
    }

    /// The quick-start plaque, opened in place.
    fn plaque() -> dc_app::App {
        fixture("plaque.dcv")
    }

    /// `tests/fixtures/<name>`, opened in place.
    fn fixture(name: &str) -> dc_app::App {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        let bytes = std::fs::read(&path).unwrap_or_default();
        let new = dc_app::App::new_project().unwrap_or_else(|e| panic!("{e}"));
        let mut app = dc_app::App::new(new, dc_app::Strings::english(), dc_app::Prefs::default());
        app.open(&bytes, None)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        app
    }

    /// Slice 27 (#619; capability 67): with the Panel marked, every
    /// outline that is not one of its targets carries the faded bit and
    /// its targets' do not, and the Targets layer holds the targets'
    /// outlines alone, in the operation's colour, in the halo's rows for
    /// one that is also selected. A target being dragged is the selection
    /// layer's. With nothing marked nothing fades and the layer is empty.
    #[test]
    #[allow(clippy::panic, reason = "test code reports failure by panicking")]
    fn marked_targets_fade_the_rest() {
        let mut app = plaque();
        let tolerance = tolerance_mm(band(4.0));
        let id = |s: &str| InstanceId::new(s);
        let count = |app: &dc_app::App, ids: &[InstanceId]| {
            let rows = (StyleId::Selected, StyleId::SelectedOpen);
            highlighted(app.scene(), ids, tolerance, rows, None).len()
        };
        let all = shapes(app.scene(), tolerance, &[]);
        assert!(all.iter().all(|s| !s.style.is_faded()));
        assert!(targets(app.scene(), tolerance, &[], &[]).is_empty());

        app.edit(Some(dc_project::OperationId::new("op1")));
        let panel = [id("i2"), id("i3")];
        let marked = shapes(app.scene(), tolerance, &[]);
        assert_eq!(marked.len(), all.len(), "the same outlines");
        let plain = marked.iter().filter(|s| !s.style.is_faded()).count();
        assert_eq!(plain, count(&app, &panel), "only the targets stay whole");
        for (was, is) in all.iter().zip(&marked) {
            assert_eq!(was.style.row(), is.style.row(), "a fade keeps the row");
        }

        let layer = targets(app.scene(), tolerance, &[], &[]);
        assert_eq!(layer.len(), count(&app, &panel));
        assert!(layer.iter().all(|s| s.style.row() == StyleId::Target as u32
            && s.style.colour_slot() == 3
            && !s.style.is_faded()));

        // The island selected too: its outline in the halo's row.
        let haloed = targets(app.scene(), tolerance, &[id("i3")], &[]);
        let halo = haloed
            .iter()
            .filter(|s| s.style.row() == StyleId::TargetHalo as u32)
            .count();
        assert_eq!(halo, count(&app, &[id("i3")]));
        assert_eq!(haloed.len(), layer.len());

        // The island dragged: the selection layer draws it.
        let dragged = targets(app.scene(), tolerance, &[id("i3")], &[id("i3")]);
        assert_eq!(dragged.len(), count(&app, &[id("i2")]));

        // An open target is dashed like any open shape.
        app.edit(Some(dc_project::OperationId::new("op5")));
        let engraved = targets(app.scene(), tolerance, &[], &[]);
        assert!(
            engraved
                .iter()
                .any(|s| s.style.row() == StyleId::TargetOpen as u32)
        );
        assert!(engraved.iter().all(|s| s.style.colour_slot() == 2));
    }

    /// Runs every pending job of `app` on this thread, as the worker does.
    fn generate(app: &mut dc_app::App) {
        let estimate = dc_job::EstimateConfig::default();
        while let Some(work) = app.next_work() {
            let result = dc_app::generation::execute(app.project(), work, "2026-10-15", &estimate);
            app.accept(result);
        }
    }

    /// Acceptance 87's clause "the stale toolpaths show faded", on the
    /// Viewport's draw list. On the generated plaque no segment of any
    /// operation's toolpath carries the stale bit. After an edit every
    /// generator reads (the stock widened) each operation keeps its old
    /// runs, so its toolpath is still drawn, and every segment of it
    /// carries the stale bit, which the shader draws at 35 % alpha
    /// (`shader.wgsl`; `renderer.rs`'s rendered test counts the pixels);
    /// generated again, none does.
    #[test]
    #[allow(clippy::panic, reason = "test code reports failure by panicking")]
    fn a_stale_operations_toolpath_is_drawn_faded() {
        let mut app = plaque();
        generate(&mut app);
        let zoom = band(4.0);
        let drawn = |app: &dc_app::App| -> Vec<(bool, Vec<Segment>)> {
            let visibility = app.view().visibility;
            let look = ToolpathLook {
                visibility: &visibility,
                ramp: None,
                tolerance_mm: tolerance_mm(zoom),
                lod_mm: lod_mm(zoom),
                male_offset: None,
            };
            app.scene()
                .operations()
                .iter()
                .map(|op| (op.stale, toolpath(op, look)))
                .collect()
        };

        let fresh = drawn(&app);
        assert_eq!(fresh.len(), 5, "the plaque's five operations");
        for (stale, segments) in &fresh {
            assert!(!stale && !segments.is_empty());
            assert!(segments.iter().all(|s| !s.style.stale()));
        }

        let mut stock = app.project().stock.clone();
        stock.width_mm += 5.0;
        app.run(dc_app::Command::SetStock(stock))
            .unwrap_or_else(|e| panic!("{e}"));
        let waiting = drawn(&app);
        assert_eq!(waiting.len(), fresh.len(), "the old drawings stay");
        for ((stale, segments), (_, was)) in waiting.iter().zip(&fresh) {
            assert!(stale, "every operation waits for its new toolpath");
            assert_eq!(segments.len(), was.len(), "the same moves");
            assert!(segments.iter().all(|s| s.style.stale()), "each drawn faded");
            for (now, before) in segments.iter().zip(was) {
                assert_eq!(now.style.row(), before.style.row(), "a fade keeps the row");
            }
        }

        generate(&mut app);
        for (stale, segments) in drawn(&app) {
            assert!(!stale && !segments.is_empty());
            assert!(segments.iter().all(|s| !s.style.stale()));
        }
    }

    /// Capabilities 73 and 70 (slice D9), on the Viewport's draw list:
    /// every coloured segment of an operation's toolpath, whichever of
    /// its tool runs it belongs to, carries the operation's colour, which
    /// is its row's; a clearing pass's carry the lighter bit besides,
    /// which the shader draws at 55 %. On the sign the Background V-Carve
    /// is the second row and two runs, so its V-bit's lines and its
    /// clearing rings are both the second colour, the rings lighter, and
    /// the Lettering after it is the third: a colour per run made the
    /// rings the third and the Lettering the fourth. The Inlay of the
    /// tapered ball nose's project, the second row after an unticked
    /// first, draws both boards in the second colour.
    #[test]
    #[allow(clippy::panic, reason = "test code reports failure by panicking")]
    fn every_run_draws_in_its_operations_colour_and_a_clearing_pass_lighter() {
        let zoom = band(4.0);
        let drawn = |app: &dc_app::App| -> Vec<(String, u32, Vec<Segment>)> {
            let visibility = app.view().visibility;
            let look = ToolpathLook {
                visibility: &visibility,
                ramp: None,
                tolerance_mm: tolerance_mm(zoom),
                lod_mm: lod_mm(zoom),
                male_offset: app.scene().board_offset(Board::Male),
            };
            let rows = dc_app::operations::rows(app);
            app.scene()
                .operations()
                .iter()
                .map(|op| {
                    let row = rows
                        .iter()
                        .find(|r| r.id == op.id)
                        .unwrap_or_else(|| panic!("{} has a row", op.name));
                    let slot = u32::try_from(row.colour % 8).unwrap_or(0) + 1;
                    (op.name.clone(), slot, toolpath(op, look))
                })
                .collect()
        };

        let mut sign = fixture("sign.dcv");
        generate(&mut sign);
        let operations = drawn(&sign);
        assert_eq!(operations.len(), 5);
        for (name, slot, segments) in &operations {
            let coloured: Vec<&Segment> = segments
                .iter()
                .filter(|s| s.style.colour_slot() != 0)
                .collect();
            assert!(!coloured.is_empty(), "{name} draws cuts");
            for segment in &coloured {
                assert_eq!(
                    segment.style.colour_slot(),
                    *slot,
                    "{name}: a segment in another row's colour"
                );
            }
            let lighter = coloured.iter().filter(|s| s.style.is_lighter()).count();
            if name == "Background" {
                assert_eq!(*slot, 2, "the second row");
                assert!(
                    lighter > 0 && lighter < coloured.len(),
                    "{name}: its clearing pass lighter, its V-bit's lines not: {lighter} of {}",
                    coloured.len()
                );
            } else {
                assert_eq!(lighter, 0, "{name} has no clearing pass");
            }
            assert!(
                segments
                    .iter()
                    .filter(|s| s.style.colour_slot() == 0)
                    .all(|s| !s.style.is_lighter()),
                "{name}: only the operation's colour is lightened"
            );
        }

        let mut inlay = fixture("dc-tbn.dcv");
        generate(&mut inlay);
        let operations = drawn(&inlay);
        let [(name, slot, segments)] = operations.as_slice() else {
            panic!("the Inlay alone is ticked: {}", operations.len());
        };
        assert_eq!((name.as_str(), *slot), ("DC inlay", 2));
        let stock_width = f64::from(gpu_f32(inlay.project().stock.width_mm));
        let on_male = |s: &&Segment| f64::from(s.a_mm[0]) > stock_width;
        let coloured = || segments.iter().filter(|s| s.style.colour_slot() != 0);
        assert!(coloured().any(|s| on_male(&s)) && coloured().any(|s| !on_male(&s)));
        assert!(
            coloured().all(|s| s.style.colour_slot() == 2),
            "both boards in the Inlay's colour"
        );
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
