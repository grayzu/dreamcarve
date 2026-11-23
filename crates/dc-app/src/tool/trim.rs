// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Trim tool (`docs/specs/phase-5/design.md`, "Algorithms →
//! Intersections and Trim"; functional capability 96): the part of an
//! outline under the pointer, between the two nearest crossings with
//! other outlines or with itself, is highlighted, and a click removes
//! it.
//!
//! The trimming is `dc_geometry::trim`, on the outlines as they are
//! placed, so shapes placed differently trim each other; what is left
//! goes back into the trimmed shape's own space. A closed outline opens;
//! an open one shortens, or splits in two shapes when it is one outline
//! of its own with no linked copy (else its shape gains an outline, so
//! the copies follow). A shape that keeps its sizes, and a path with
//! curves, becomes a plain path with the trimmed outline, in the one
//! undo step.
//!
//! PureCutCNC's trim is two picks on open profiles, a subject and a
//! reference, and cannot take a middle piece out; this is one click on
//! any outline, written from the functional spec.

use dc_geometry::{Point, Polyline, Trimmed, nearest_point, trim};
use dc_project::InstanceId;

use super::{Draft, Event, Key, Reply, ToolBar};
use crate::command::{Edit, NewShape, copy_name};
use crate::snap::Pointer;
use crate::strings::StrKey;
use crate::viewport::pick::PICK_TOLERANCE_PX;
use crate::{App, Command, Tool};

/// The outline under the pointer, and what trimming it there leaves.
struct Hit {
    instance: InstanceId,
    /// Which of the shape's outlines.
    profile: usize,
    /// The trim, in project space; `None` when nothing crosses the
    /// outline.
    trimmed: Option<Trimmed>,
}

impl App {
    /// The outline within reach of project point `at`, nearest first,
    /// trimmed there against every other visible outline.
    fn trim_hit(&self, at: Point) -> Option<Hit> {
        let px_per_mm = self.view().camera.px_per_mm;
        let scene = self.scene();
        let (shape, profile, _) = scene
            .shapes()
            .iter()
            .flat_map(|shape| {
                shape
                    .outlines
                    .iter()
                    .enumerate()
                    .filter_map(move |(profile, outline)| {
                        let gap_px = nearest_point(outline, at)?.gap_mm * px_per_mm;
                        (gap_px <= PICK_TOLERANCE_PX).then_some((shape, profile, gap_px))
                    })
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))?;
        let outline = shape.outlines.get(profile)?;
        let others: Vec<Polyline> = scene
            .shapes()
            .iter()
            .flat_map(|other| {
                other
                    .outlines
                    .iter()
                    .enumerate()
                    .filter(move |(index, _)| other.instance != shape.instance || *index != profile)
                    .map(|(_, outline)| outline.clone())
            })
            .collect();
        Some(Hit {
            instance: shape.instance.clone(),
            profile,
            trimmed: trim(outline, at, &others),
        })
    }

    /// Hands `event` to the Trim tool.
    pub(super) fn trim_event(&mut self, event: Event) -> Reply {
        let reply = match event {
            Event::Move(pointer) => {
                self.tool.pointer = Some(pointer);
                Reply::default()
            }
            Event::Leave => {
                self.tool.pointer = None;
                Reply::default()
            }
            Event::Press(pointer) => {
                self.tool.pointer = Some(pointer);
                self.trim_click(&pointer)
            }
            Event::Key(Key::Escape | Key::Enter) | Event::Done => {
                self.arm(Tool::Select);
                return Reply::default();
            }
            _ => Reply::default(),
        };
        // The piece a click would remove.
        let mut draft = Draft::default();
        if let Some(pointer) = self.tool.pointer
            && let Some(Hit {
                trimmed: Some(trimmed),
                ..
            }) = self.trim_hit(pointer.at_mm)
            && trimmed.removed.vertices().len() >= 2
        {
            draft.highlights.push(trimmed.removed);
        }
        self.tool.draft = draft;
        reply
    }

    /// A click: the piece under the pointer goes.
    fn trim_click(&mut self, pointer: &Pointer) -> Reply {
        let Some(hit) = self.trim_hit(pointer.at_mm) else {
            return Reply::default();
        };
        let refused = |reason: String| Reply {
            ran: None,
            refused: Some(reason),
        };
        let Some(trimmed) = hit.trimmed else {
            return refused(self.strings().get(StrKey::TrimRefusedNoCrossing).to_owned());
        };
        let project = self.project();
        let Some(instance) = project.instances.iter().find(|i| i.id == hit.instance) else {
            return Reply::default();
        };
        let Some(definition) = project.definitions.get(&instance.definition) else {
            return Reply::default();
        };
        let Some(to_definition) = instance.transform.inverse() else {
            return Reply::default();
        };
        // What is left, back in the shape's own space.
        let kept: Result<Vec<Polyline>, _> = trimmed
            .kept
            .iter()
            .map(|piece| to_definition.apply_polyline(piece))
            .collect();
        let kept = match kept {
            Ok(kept) => kept,
            Err(error) => return refused(error.to_string()),
        };
        let linked = project
            .instances
            .iter()
            .filter(|i| i.definition == instance.definition)
            .count()
            > 1;
        let command = match kept.as_slice() {
            // Two pieces of a shape that is one outline of its own: two
            // shapes, the first with what cut it.
            [first, second] if definition.profiles.len() == 1 && !linked => {
                let taken: Vec<&str> = project.instances.iter().map(|i| i.name.as_str()).collect();
                let part = |name: String, outline: &Polyline| NewShape {
                    name,
                    shape: None,
                    profiles: vec![outline.clone()],
                    transform: instance.transform,
                    construction: instance.construction,
                    folder: instance.folder.clone(),
                };
                Command::ReplaceShapes {
                    from: vec![instance.id.clone()],
                    keep: false,
                    add: vec![
                        part(instance.name.clone(), first),
                        part(copy_name(&taken, &instance.name), second),
                    ],
                    inherit: vec![(instance.id.clone(), 0)],
                    what: Edit::Trim,
                }
            }
            // Else the shape's outline changes: the trimmed one replaced
            // by what is left of it.
            _ => {
                let mut profiles = definition.profiles.clone();
                let at = hit.profile.min(profiles.len());
                if at < profiles.len() {
                    profiles.splice(at..=at, kept);
                }
                Command::SetOutline {
                    instance: instance.id.clone(),
                    shape: None,
                    profiles,
                    what: Edit::Trim,
                }
            }
        };
        match self.run(command) {
            Ok(label) => Reply {
                ran: Some(label),
                refused: None,
            },
            Err(error) => refused(error.to_string()),
        }
    }

    /// The tool bar for the Trim tool.
    pub(super) fn trim_bar(&self) -> ToolBar {
        ToolBar {
            tool: Tool::Trim,
            prompt: self.strings().get(StrKey::TrimPrompt).to_owned(),
            fields: Vec::new(),
            choices: Vec::new(),
            actions: Vec::new(),
            note: None,
            can_finish: true,
        }
    }
}
