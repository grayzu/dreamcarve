// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/platform/featureClipboard.ts

//! Cut, copy and paste of shapes (functional spec, capability 98).
//!
//! The clipboard is the app's, not the system's: it holds each copied
//! shape with its definition, so what was cut pastes after the project
//! has let its definition go, and it is kept when another project is
//! opened in the same window, so shapes carry from one project to
//! another. A paste is shapes of their own, not linked copies.
//!
//! As in PureCutCNC, what is pasted at a point has the middle of its
//! joint bounding box there (`featureClipboardAnchor`). Where it leaves
//! PureCutCNC: a hidden shape that is selected is copied too, and pasted
//! visible; a paste with no point lands 10 mm off the originals and
//! 10 mm further each time, and what was cut pastes first where it was.

use dc_geometry::{Point, Rect, Transform};
use dc_project::{FeatureDefinition, FeatureInstance};

use crate::app::App;
use crate::command::Command;
use crate::error::CommandError;

/// How far a paste with no point lands from the originals, and from the
/// paste before it, along X and Y: a duplicate's offset.
pub const PASTE_OFFSET_MM: f64 = 10.0;

/// One copied shape: the instance as it was placed, with the definition
/// it placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// The shape's outlines and sizes.
    pub definition: FeatureDefinition,
    /// Its name, placement and marks.
    pub instance: FeatureInstance,
}

/// What was last cut or copied.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Clipboard {
    clips: Vec<Clip>,
    /// The clips' joint bounding box as they were placed.
    bounds: Option<Rect>,
    /// How many offsets the next paste with no point lands from the
    /// originals: one after a copy, none after a cut, one more with each
    /// such paste.
    steps: u32,
}

impl Clipboard {
    /// The shapes held, in the Design Tree's order when they were copied.
    #[must_use]
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// Whether nothing is held.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }
}

impl App {
    /// Whether **Paste** has something to paste.
    #[must_use]
    pub const fn can_paste(&self) -> bool {
        !self.clipboard.is_empty()
    }

    /// **Copy**: the selected shapes go to the clipboard, in the Design
    /// Tree's order. The project is not changed. Returns how many were
    /// copied; with no shape selected the clipboard keeps what it held.
    pub fn copy_selection(&mut self) -> usize {
        self.fill_clipboard(1)
    }

    /// **Cut**: the selected shapes go to the clipboard and leave the
    /// project, as Delete removes them, in one undo step.
    ///
    /// # Errors
    ///
    /// [`CommandError::NothingSelected`] with no shape selected, or the
    /// command's refusal; nothing changes, the clipboard included.
    pub fn cut_selection(&mut self) -> Result<String, CommandError> {
        let ids = self.selection().instances().to_vec();
        if ids.is_empty() {
            return Err(CommandError::NothingSelected);
        }
        let held = self.clipboard.clone();
        self.fill_clipboard(0);
        let ran = self.run(Command::CutInstances(ids));
        if ran.is_err() {
            self.clipboard = held;
        }
        ran
    }

    /// **Paste**: the clipboard's shapes as new shapes of their own,
    /// selected, in one undo step. With `at_mm`, the middle of what was
    /// copied goes there; with none, each lands [`PASTE_OFFSET_MM`] off
    /// where it was copied from, and that much further with each paste.
    ///
    /// # Errors
    ///
    /// [`CommandError::NothingToDo`] when the clipboard is empty, or the
    /// command's refusal; nothing changes.
    pub fn paste(&mut self, at_mm: Option<Point>) -> Result<String, CommandError> {
        let moved = match (at_mm, self.clipboard.bounds) {
            (Some(at), Some(bounds)) => {
                let centre_x = f64::midpoint(bounds.min.x, bounds.max.x);
                let centre_y = f64::midpoint(bounds.min.y, bounds.max.y);
                Transform::translate(at.x - centre_x, at.y - centre_y)
            }
            _ => {
                let off = PASTE_OFFSET_MM * f64::from(self.clipboard.steps);
                Transform::translate(off, off)
            }
        };
        let clips: Vec<Clip> = self
            .clipboard
            .clips
            .iter()
            .map(|clip| Clip {
                definition: clip.definition.clone(),
                instance: FeatureInstance {
                    transform: clip.instance.transform.then(moved),
                    ..clip.instance.clone()
                },
            })
            .collect();
        let label = self.run(Command::PasteShapes(clips))?;
        if at_mm.is_none() {
            self.clipboard.steps = self.clipboard.steps.saturating_add(1);
        }
        Ok(label)
    }

    /// Fills the clipboard from the selection, the next paste with no
    /// point `steps` offsets off; how many shapes it took.
    fn fill_clipboard(&mut self, steps: u32) -> usize {
        let project = self.project();
        let selected = self.selection().instances();
        let clips: Vec<Clip> = project
            .instances
            .iter()
            .filter(|instance| selected.contains(&instance.id))
            .filter_map(|instance| {
                Some(Clip {
                    definition: project.definitions.get(&instance.definition)?.clone(),
                    instance: instance.clone(),
                })
            })
            .collect();
        if clips.is_empty() {
            return 0;
        }
        let mut bounds: Option<Rect> = None;
        for clip in &clips {
            for profile in &clip.definition.profiles {
                let placed = clip
                    .instance
                    .transform
                    .apply_polyline(profile)
                    .ok()
                    .and_then(|outline| outline.bounds());
                bounds = match (bounds, placed) {
                    (Some(a), Some(b)) => Some(a.including(b.min).including(b.max)),
                    (a, b) => a.or(b),
                };
            }
        }
        let count = clips.len();
        self.clipboard = Clipboard {
            clips,
            bounds,
            steps,
        };
        count
    }
}
