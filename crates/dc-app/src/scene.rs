// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Scene (`docs/specs/phase-4/design.md`, "Data model → dc-app"): the
//! read model the Viewport draws and [`pick`](crate::viewport::pick) reads.
//! Every visible instance's outlines in project space, placed by the
//! instance's transform exactly as the generators and the preview place
//! them (`Transform::apply_polyline`), each placed tab's project-space
//! point with the enabled Profiles that cross it (none draws it hollow,
//! slice 17), and each enabled operation's generation state. The app rebuilds
//! it when the snapshot or a generation result changes, never per frame,
//! so selecting, panning and zooming cost nothing here.
//!
//! Each enabled operation carries the runs of its stored result (slice
//! 16): the generator's toolpaths as they came back from the worker, the
//! marks [`dc_job::preview::marks`] finds in each (the ones `--preview`
//! draws, acceptance 79), and each run's place in the preview's palette.
//! A stale operation keeps its last result's runs, which the Viewport
//! draws faded, until the new one lands (acceptance 87). The runs are
//! shared, not copied, between Scenes while their result stands.
//!
//! [`strokes`] reads a toolpath as the Viewport draws it: one stroke per
//! move, a rapid, a cut or a plunge, with arcs kept; display code
//! flattens and coalesces them (`dreamcarve-app::viewport::lod`), never
//! the toolpath itself.
//!
//! Not yet: bitmaps (no picture type before phase 3.4; slices 6 and 15)
//! and the male board (slice 14), whose runs the Scene holds for the
//! preview's SVG but the Viewport does not draw yet.

use std::sync::Arc;

use dc_geometry::{
    EPS_MM, Point, Point3, Polyline, Rect, Segment, Vec2, point_segment_distance_mm,
};
use dc_job::preview::{self, Marks};
use dc_job::{EmitOp, MoveKind, Toolpath};
use dc_project::{InstanceId, OperationId, Project, TabId};

use crate::generation::{Generation, SnapshotId};

/// The tolerance the Scene flattens outlines to for picking and the
/// marquee, mm: well under a pixel at any zoom a click is aimed at.
pub const PICK_FLATTEN_MM: f64 = 0.01;

/// One visible instance, placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// The instance.
    pub instance: InstanceId,
    /// Its name, for the status bar's hover readout.
    pub name: String,
    /// Locked: drawn grey with a padlock, picked but not transformed.
    pub locked: bool,
    /// Its definition's profiles in project space, mm, arcs kept.
    pub outlines: Vec<Polyline>,
    /// `outlines` with every arc flattened to [`PICK_FLATTEN_MM`].
    pub flat: Vec<Polyline>,
    /// The outlines' bounding box, mm; `None` when they have no vertex.
    pub bounds: Option<Rect>,
}

/// A placed tab, in project space.
#[derive(Debug, Clone, PartialEq)]
pub struct TabMark {
    /// The tab.
    pub id: TabId,
    /// Its centre placed by its instance's transform, mm.
    pub at_mm: Point,
    /// The instance whose outline holds it.
    pub instance: InstanceId,
    /// Its width along the outline, mm.
    pub width_mm: f64,
    /// The outline's direction at the point of it nearest the tab, a unit
    /// vector in project space: the tab's span runs along it.
    pub tangent: Vec2,
    /// The enabled Profiles that cross it, in operation order
    /// (`dc_job::tabs::crossed_by`, slice 17); empty draws it hollow.
    pub crossed_by: Vec<OperationId>,
}

impl TabMark {
    /// Whether no enabled Profile crosses it: the Viewport draws it hollow.
    #[must_use]
    pub fn hollow(&self) -> bool {
        self.crossed_by.is_empty()
    }
}

/// One tool run of an operation, as the Viewport draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneRun {
    /// The run as generated: operation, tool, toolpath, board.
    pub run: EmitOp,
    /// Its tab marks, start points and holes, as `--preview` draws them.
    pub marks: Marks,
}

/// An enabled operation's generation state and its last result's runs.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneOperation {
    /// The operation.
    pub id: OperationId,
    /// Its name, for the depth legend.
    pub name: String,
    /// Its position in the project's list.
    pub index: usize,
    /// Whether its stored result is not the current snapshot's (or it has
    /// none yet): drawn faded until the new one lands.
    pub stale: bool,
    /// The preview palette's index of its first run: the runs of every
    /// enabled operation before it, counted as `--preview` counts them
    /// (`dc_job::preview::render`, one colour per run, repeating after
    /// eight). Its `k`th run is colour `colour + k`.
    pub colour: usize,
    /// Its stored result's runs, in cut order; empty before the first
    /// result and after a generation error.
    pub runs: Arc<[SceneRun]>,
    /// Its runs' cutting moves as clicks pick them (#576), built with
    /// `runs` and shared with them.
    pub cuts: Arc<CutIndex>,
    /// Which result `runs` are: its snapshot and digest.
    result: Option<(SnapshotId, u64)>,
}

impl SceneOperation {
    /// Which result its runs are, as a cache key: it changes when a new
    /// result replaces them, and on nothing else.
    #[must_use]
    pub fn result_key(&self) -> Option<(SnapshotId, u64)> {
        self.result
    }
}

/// What the Viewport shows of one snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    revision: u64,
    shapes_revision: u64,
    shapes: Vec<Shape>,
    tabs: Vec<TabMark>,
    operations: Vec<SceneOperation>,
    bounds: Option<Rect>,
}

/// How many lines one chunk of a [`CutIndex`] holds: consecutive moves
/// lie close together, so a chunk's box is small and most chunks are
/// passed over by their box alone.
const CHUNK_LINES: usize = 64;

/// One run of consecutive cutting lines and the box around them.
#[derive(Debug, Clone, PartialEq)]
struct CutChunk {
    bounds: Rect,
    lines: Vec<(Point, Point)>,
}

/// An operation's cutting moves on the female board as a click picks them
/// (#576): every cut the Viewport draws, arcs flattened to
/// [`PICK_FLATTEN_MM`], in chunks of consecutive lines with their boxes.
/// Built once per result, when the Scene takes its runs, so a hover or a
/// click only measures the chunks whose box it falls near.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CutIndex {
    chunks: Vec<CutChunk>,
}

impl CutIndex {
    /// The index of `runs`' cutting moves. The male board's runs are left
    /// out, as the Viewport does not draw them yet (slice 14).
    #[must_use]
    pub fn of(runs: &[SceneRun]) -> Self {
        let mut lines: Vec<(Point, Point)> = Vec::new();
        for run in runs
            .iter()
            .filter(|r| r.run.board != dc_project::Board::Male)
        {
            for stroke in strokes(&run.run.toolpath) {
                if stroke.kind != StrokeKind::Cut {
                    continue;
                }
                let points = stroke.points(PICK_FLATTEN_MM);
                for pair in points.windows(2) {
                    if let [a, b] = pair
                        && xy(*a).distance_to(xy(*b)) > XY_EPS_MM
                    {
                        lines.push((xy(*a), xy(*b)));
                    }
                }
            }
        }
        let chunks = lines
            .chunks(CHUNK_LINES)
            .filter_map(|chunk| {
                let (first, _) = chunk.first()?;
                let bounds = chunk.iter().fold(
                    Rect {
                        min: *first,
                        max: *first,
                    },
                    |b, (a, c)| b.including(*a).including(*c),
                );
                Some(CutChunk {
                    bounds,
                    lines: chunk.to_vec(),
                })
            })
            .collect();
        Self { chunks }
    }

    /// Whether it holds no cut.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// The distance from `p` to the nearest cut, mm, when one is within
    /// `within_mm`.
    #[must_use]
    pub fn distance_mm(&self, p: Point, within_mm: f64) -> Option<f64> {
        self.chunks
            .iter()
            .filter(|c| {
                p.x >= c.bounds.min.x - within_mm
                    && p.x <= c.bounds.max.x + within_mm
                    && p.y >= c.bounds.min.y - within_mm
                    && p.y <= c.bounds.max.y + within_mm
            })
            .flat_map(|c| c.lines.iter())
            .map(|(start, end)| {
                point_segment_distance_mm(
                    Segment {
                        start: *start,
                        end: *end,
                        bulge: 0.0,
                    },
                    p,
                )
            })
            .filter(|d| *d <= within_mm)
            .reduce(f64::min)
    }
}

/// Each enabled operation of `project` with `generation`'s state and its
/// stored runs, those of `previous` reused where their result stands.
fn operations(
    project: &Project,
    generation: &Generation,
    previous: &[SceneOperation],
) -> Vec<SceneOperation> {
    let mut colour = 0;
    project
        .operations
        .iter()
        .enumerate()
        .filter(|(_, op)| op.enabled())
        .map(|(index, op)| {
            let stored = generation.result(op.id());
            let result = stored.map(|r| (r.snapshot, r.digest));
            let kept = previous
                .iter()
                .find(|p| p.id == *op.id() && p.result == result && result.is_some());
            let (runs, cuts): (Arc<[SceneRun]>, Arc<CutIndex>) = match (kept, stored) {
                (Some(kept), _) => (Arc::clone(&kept.runs), Arc::clone(&kept.cuts)),
                (None, Some(stored)) => {
                    let runs: Arc<[SceneRun]> = stored
                        .runs
                        .iter()
                        .enumerate()
                        .map(|(k, run)| SceneRun {
                            marks: stored
                                .marks
                                .get(k)
                                .cloned()
                                .unwrap_or_else(|| preview::marks(&preview::Run::from(run))),
                            run: run.clone(),
                        })
                        .collect();
                    let cuts = Arc::new(CutIndex::of(&runs));
                    (runs, cuts)
                }
                (None, None) => (Arc::from(Vec::new()), Arc::default()),
            };
            let scene_op = SceneOperation {
                id: op.id().clone(),
                name: op.name().to_owned(),
                index,
                stale: !generation.is_current(op.id()),
                colour,
                runs,
                cuts,
                result,
            };
            colour += scene_op.runs.len();
            scene_op
        })
        .collect()
}

/// Every placed tab of `project` whose instance it has, placed, with the
/// outline's direction at it and the enabled Profiles that cross it.
fn tab_marks(project: &Project) -> Vec<TabMark> {
    let mut crossings = crate::viewport::tabs::crossings(project);
    project
        .tabs
        .iter()
        .filter_map(|tab| {
            let (at_mm, tangent) = crate::viewport::tabs::tab_frame(project, tab)?;
            Some(TabMark {
                id: tab.id.clone(),
                at_mm,
                instance: tab.instance.clone(),
                width_mm: tab.width_mm,
                tangent,
                crossed_by: crossings.remove(&tab.id).unwrap_or_default(),
            })
        })
        .collect()
}

/// The joint box of `a` and `b`.
fn join(a: Option<Rect>, b: Rect) -> Rect {
    a.map_or(b, |a| a.including(b.min).including(b.max))
}

impl Scene {
    /// The Scene of `project` with `generation`'s state, numbered
    /// `revision`. An instance whose definition is missing or whose
    /// transform cannot place a profile is left out (validation reports
    /// it); a hidden one is left out (capability 72: "hidden absent").
    #[must_use]
    pub fn build(project: &Project, generation: &Generation, revision: u64) -> Self {
        Self::rebuild(project, generation, revision, &Self::default())
    }

    /// [`Scene::build`] for a new snapshot, the runs of `previous` kept
    /// where their result stands, so an edit does not copy every
    /// operation's toolpaths again.
    pub(crate) fn rebuild(
        project: &Project,
        generation: &Generation,
        revision: u64,
        previous: &Self,
    ) -> Self {
        let mut shapes = Vec::with_capacity(project.instances.len());
        let mut bounds = None;
        for instance in project.instances.iter().filter(|i| i.visible) {
            let Some(definition) = project.definitions.get(&instance.definition) else {
                continue;
            };
            let Ok(outlines) = definition
                .profiles
                .iter()
                .map(|p| instance.transform.apply_polyline(p))
                .collect::<Result<Vec<_>, _>>()
            else {
                continue;
            };
            let flat = outlines
                .iter()
                .map(|o| o.with_arcs_flattened(PICK_FLATTEN_MM))
                .collect();
            let shape_bounds = outlines
                .iter()
                .filter_map(Polyline::bounds)
                .fold(None, |acc, b| Some(join(acc, b)));
            if let Some(b) = shape_bounds {
                bounds = Some(join(bounds, b));
            }
            shapes.push(Shape {
                instance: instance.id.clone(),
                name: instance.name.clone(),
                locked: instance.locked,
                outlines,
                flat,
                bounds: shape_bounds,
            });
        }
        let tabs = tab_marks(project);
        Self {
            revision,
            shapes_revision: revision,
            shapes,
            tabs,
            operations: operations(project, generation, &previous.operations),
            bounds,
        }
    }

    /// The Scene after a generation result, Stop or Regenerate all on the
    /// same snapshot: the operations' state re-read and the runs of a
    /// result that stands kept, the shapes kept (on
    /// 500 shapes, placing every outline again would cost a frame), under
    /// `revision`.
    pub(crate) fn refresh_operations(
        &mut self,
        project: &Project,
        generation: &Generation,
        revision: u64,
    ) {
        self.operations = operations(project, generation, &self.operations);
        self.revision = revision;
    }

    /// Which build this is: it changes on every rebuild and every result
    /// and on nothing else.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Which build of the shapes and tabs this is: it changes only when
    /// the snapshot does, so the renderer re-uploads the shape layers
    /// only then, not for every generation result.
    #[must_use]
    pub fn shapes_revision(&self) -> u64 {
        self.shapes_revision
    }

    /// The visible instances in file order: the last is drawn on top.
    #[must_use]
    pub fn shapes(&self) -> &[Shape] {
        &self.shapes
    }

    /// The visible instance `id`, if it is in the Scene.
    #[must_use]
    pub fn shape(&self, id: &InstanceId) -> Option<&Shape> {
        self.shapes.iter().find(|s| s.instance == *id)
    }

    /// The placed tabs.
    #[must_use]
    pub fn tabs(&self) -> &[TabMark] {
        &self.tabs
    }

    /// The placed tab `id`, if it is in the Scene.
    #[must_use]
    pub fn tab(&self, id: &TabId) -> Option<&TabMark> {
        self.tabs.iter().find(|t| t.id == *id)
    }

    /// The enabled operations, in list order.
    #[must_use]
    pub fn operations(&self) -> &[SceneOperation] {
        &self.operations
    }

    /// The enabled operation `id`, if it is in the Scene.
    #[must_use]
    pub fn operation(&self, id: &OperationId) -> Option<&SceneOperation> {
        self.operations.iter().find(|o| o.id == *id)
    }

    /// Every stored run of every enabled operation, in list order: what
    /// `--preview` draws, once every operation is current.
    pub fn runs(&self) -> impl Iterator<Item = &EmitOp> {
        self.operations
            .iter()
            .flat_map(|o| o.runs.iter().map(|r| &r.run))
    }

    /// The Viewport's SVG dump: `--preview`'s picture of the stored runs
    /// (`dc_job::preview::render`; acceptance 79). Equal to the CLI's file
    /// for the saved project once every operation is current.
    #[must_use]
    pub fn preview_svg(&self, project: &Project) -> String {
        let runs: Vec<preview::Run<'_>> = self.runs().map(preview::Run::from).collect();
        preview::render(project, &runs)
    }

    /// The joint box of every visible shape, mm.
    #[must_use]
    pub fn shapes_bounds(&self) -> Option<Rect> {
        self.bounds
    }

    /// The joint box of the visible shapes among `ids`, mm (Zoom to
    /// selection).
    #[must_use]
    pub fn bounds_of(&self, ids: &[InstanceId]) -> Option<Rect> {
        ids.iter()
            .filter_map(|id| self.shape(id)?.bounds)
            .fold(None, |acc, b| Some(join(acc, b)))
    }
}

/// What a stroke of a toolpath is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeKind {
    /// A rapid: thin, grey, dashed.
    Rapid,
    /// A cutting move with travel in XY, or one that rises: solid in the
    /// operation's colour.
    Cut,
    /// A cutting move straight down: a plunge mark at its XY.
    Plunge,
}

/// One move of a toolpath as the Viewport draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    /// What it is.
    pub kind: StrokeKind,
    /// Where the tool was, mm.
    pub from: Point3,
    /// Where the move takes it, mm.
    pub to: Point3,
    /// For an arc, its centre and whether it turns counter-clockwise
    /// (viewed from +Z, Y-up).
    pub arc: Option<(Point, bool)>,
}

/// The fewest chords a flattened arc takes, so a full turn stays round.
const MIN_ARC_CHORDS: usize = 4;
/// The most chords a flattened arc takes.
const MAX_ARC_CHORDS: usize = 4096;

impl Stroke {
    /// The stroke's path, mm, from `from` to `to`: an arc flattened so no
    /// chord strays more than `tolerance_mm` from it, its Z interpolated
    /// along the sweep (a helix); a line as its two ends. An arc that ends
    /// where it starts is a full turn, as G-code reads it. Display only.
    #[must_use]
    pub fn points(&self, tolerance_mm: f64) -> Vec<Point3> {
        let Some((centre, ccw)) = self.arc else {
            return vec![self.from, self.to];
        };
        let (from, to) = (xy(self.from), xy(self.to));
        let radius = from.distance_to(centre);
        if radius <= EPS_MM {
            return vec![self.from, self.to];
        }
        let sweep = if from.distance_to(to) <= EPS_MM {
            if ccw {
                std::f64::consts::TAU
            } else {
                -std::f64::consts::TAU
            }
        } else {
            dc_geometry::Arc::from_ends(from, to, centre, ccw).sweep_rad
        };
        let tolerance = tolerance_mm.clamp(EPS_MM, radius);
        let step = 2.0 * (1.0 - tolerance / radius).acos();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a positive chord count, clamped to MAX_ARC_CHORDS"
        )]
        let chords = if step > 0.0 {
            ((sweep.abs() / step).ceil() as usize).clamp(MIN_ARC_CHORDS, MAX_ARC_CHORDS)
        } else {
            MAX_ARC_CHORDS
        };
        let start = (from.y - centre.y).atan2(from.x - centre.x);
        let mut points = Vec::with_capacity(chords + 1);
        points.push(self.from);
        for k in 1..chords {
            #[allow(clippy::cast_precision_loss, reason = "under 4096 chords")]
            let t = k as f64 / chords as f64;
            let angle = start + sweep * t;
            points.push(Point3::new(
                centre.x + radius * angle.cos(),
                centre.y + radius * angle.sin(),
                self.from.z + (self.to.z - self.from.z) * t,
            ));
        }
        points.push(self.to);
        points
    }
}

/// The XY of `p`.
const fn xy(p: Point3) -> Point {
    Point::new(p.x, p.y)
}

/// Two Z values closer than this are one level, mm (`--preview`'s).
const Z_EPS_MM: f64 = 1e-6;
/// A move shorter than this in XY has no XY extent, mm (`--preview`'s).
const XY_EPS_MM: f64 = 1e-9;

/// Every move of `toolpath` as a stroke, in order: the moves the Viewport
/// draws (acceptance 79: same count, same end points). The first move
/// only places the tool and a dwell does not move it, so neither is a
/// stroke. A cutting move with no XY extent going down is a plunge.
#[must_use]
pub fn strokes(toolpath: &Toolpath) -> Vec<Stroke> {
    let mut out = Vec::with_capacity(toolpath.moves.len());
    let mut at: Option<Point3> = None;
    for m in &toolpath.moves {
        if m.is_dwell() {
            continue;
        }
        let Some(from) = at.replace(m.to) else {
            continue;
        };
        let arc = match m.kind {
            MoveKind::ArcCw { centre } => Some((centre, false)),
            MoveKind::ArcCcw { centre } => Some((centre, true)),
            _ => None,
        };
        let kind = if m.is_rapid() {
            StrokeKind::Rapid
        } else if arc.is_none()
            && xy(from).distance_to(xy(m.to)) <= XY_EPS_MM
            && m.to.z < from.z - Z_EPS_MM
        {
            StrokeKind::Plunge
        } else {
            StrokeKind::Cut
        };
        out.push(Stroke {
            kind,
            from,
            to: m.to,
            arc,
        });
    }
    out
}
