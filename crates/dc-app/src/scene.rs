// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Scene (`docs/specs/phase-4/design.md`, "Data model → dc-app"): the
//! read model the Viewport draws and [`pick`](crate::viewport::pick) reads.
//! Every visible instance's outlines in project space, placed by the
//! instance's transform exactly as the generators and the preview place
//! them (`Transform::apply_polyline`), each placed tab's project-space
//! point, and each enabled operation's generation state. The app rebuilds
//! it when the snapshot or a generation result changes, never per frame,
//! so selecting, panning and zooming cost nothing here.
//!
//! Not yet: bitmaps (no picture type before phase 3.4; slices 6 and 15),
//! each operation's runs and marks (slice 16 reads them from the
//! generation results the Scene already follows), and the male board
//! (slice 14).

use dc_geometry::{Point, Polyline, Rect};
use dc_project::{InstanceId, OperationId, Project, TabId};

use crate::generation::Generation;

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
}

/// An enabled operation's generation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneOperation {
    /// The operation.
    pub id: OperationId,
    /// Its position in the project's list, which picks its colour.
    pub index: usize,
    /// Whether its stored result is not the current snapshot's (or it has
    /// none yet): drawn faded until the new one lands.
    pub stale: bool,
}

/// What the Viewport shows of one snapshot.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    revision: u64,
    shapes: Vec<Shape>,
    tabs: Vec<TabMark>,
    operations: Vec<SceneOperation>,
    bounds: Option<Rect>,
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
        let tabs = project
            .tabs
            .iter()
            .filter_map(|tab| {
                let instance = project.instances.iter().find(|i| i.id == tab.instance)?;
                Some(TabMark {
                    id: tab.id.clone(),
                    at_mm: instance.transform.apply(Point::new(tab.x_mm, tab.y_mm)),
                })
            })
            .collect();
        let operations = project
            .operations
            .iter()
            .enumerate()
            .filter(|(_, op)| op.enabled())
            .map(|(index, op)| SceneOperation {
                id: op.id().clone(),
                index,
                stale: !generation.is_current(op.id()),
            })
            .collect();
        Self {
            revision,
            shapes,
            tabs,
            operations,
            bounds,
        }
    }

    /// Which build this is: it changes on every rebuild and on nothing
    /// else, so the renderer re-uploads a layer only when it does.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
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

    /// The enabled operations, in list order.
    #[must_use]
    pub fn operations(&self) -> &[SceneOperation] {
        &self.operations
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
