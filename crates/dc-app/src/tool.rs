// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport's tool in use (`docs/specs/phase-5/design.md`, "Data
//! model → dc-app" and "The tool state machines"; functional capability
//! 81). One tool at a time: Select, Edit nodes, the Tab tool, Trim,
//! Measure, or a drawing tool.
//!
//! The window feeds a tool [`Event`]s, pointer positions in project
//! millimetres and keys, through [`App::tool_event`], and paints what
//! [`App::draft`] says: the shape being drawn, the snap marker, the size
//! labels. When a tool finishes a shape it runs the [`crate::Command`]
//! itself, so every drawing test runs with no window.
//!
//! A tool's state is not project data: it is not saved, not in the undo
//! history, and gone when another tool is armed.

use dc_geometry::{Point, Polyline};
use dc_project::InstanceId;

use crate::App;
use crate::snap::{self, Pointer, Snapped, Targets};

/// What the Viewport's pointer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    /// Select, move, resize and turn shapes: phase 4's Viewport.
    #[default]
    Select,
    /// Edit the nodes of the selected path (capability 92).
    Nodes,
    /// Add placed tabs to outlines (phase 4, capability 69).
    Tab,
    /// Remove the part of an outline between two crossings (capability
    /// 96).
    Trim,
    /// Measure between two points (capability 103).
    Measure,
    /// Draw a shape.
    Draw(Drawing),
}

/// A drawing tool (CONTEXT: Drawing Tool), in the order the Draw popover
/// lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Drawing {
    /// Straight segments, open or closed (capability 87).
    Line,
    /// Arc segments (capability 88).
    Arc,
    /// A smooth curve through points (capability 89).
    Spline,
    /// A rectangle, its corners square, rounded or chamfered (capability
    /// 85).
    #[default]
    Rectangle,
    /// A circle (capability 85a).
    Circle,
    /// An ellipse (capability 85a).
    Ellipse,
    /// A regular polygon (capability 85b).
    Polygon,
    /// A slot (capability 85c).
    Slot,
    /// A gear (capability 86).
    Gear,
    /// Text (capability 91).
    Text,
}

impl Drawing {
    /// Every drawing tool, in the Draw popover's order.
    pub const ALL: [Self; 10] = [
        Self::Line,
        Self::Arc,
        Self::Spline,
        Self::Rectangle,
        Self::Circle,
        Self::Ellipse,
        Self::Polygon,
        Self::Slot,
        Self::Gear,
        Self::Text,
    ];

    /// The tool's name in `gui.toml`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Arc => "arc",
            Self::Spline => "spline",
            Self::Rectangle => "rectangle",
            Self::Circle => "circle",
            Self::Ellipse => "ellipse",
            Self::Polygon => "polygon",
            Self::Slot => "slot",
            Self::Gear => "gear",
            Self::Text => "text",
        }
    }

    /// The tool named `key` in `gui.toml`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.key() == key)
    }
}

/// A key a tool reads while it is armed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Key {
    /// Finish, or commit the step with the sizes as they read.
    Enter,
    /// Step back: out of the draft, then out of the tool.
    Escape,
    /// Take back the last point.
    Backspace,
    /// Delete what is selected in the tool.
    Delete,
}

/// What the window tells the tool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The pointer moved, over the Viewport.
    Move(Pointer),
    /// The primary button went down.
    Press(Pointer),
    /// The primary button came up.
    Release(Pointer),
    /// A second click in the same place.
    DoubleClick(Pointer),
    /// A key the tool reads.
    Key(Key),
    /// The pointer left the Viewport.
    Leave,
}

/// What an event did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    /// The undo label of the change the event committed, when it
    /// committed one.
    pub ran: Option<String>,
    /// Why the event was refused, for the status bar.
    pub refused: Option<String>,
}

/// A size written beside what is being drawn (capability 83a).
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    /// Where to write it, mm.
    pub at_mm: Point,
    /// The text, in project units.
    pub text: String,
}

/// What the tool wants drawn this frame, in project millimetres.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Draft {
    /// The shape being drawn, as it would be made now.
    pub outlines: Vec<Polyline>,
    /// Where the next point would be taken, with what it snapped to.
    pub snap: Option<Snapped>,
    /// The sizes of the shape being drawn.
    pub labels: Vec<Label>,
}

/// The tool in use and what it holds. Not project data.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Session {
    pub(crate) tool: Tool,
    pub(crate) draft: Draft,
}

impl App {
    /// The tool in use.
    #[must_use]
    pub fn tool(&self) -> Tool {
        self.tool.tool
    }

    /// Arms `tool`, dropping whatever the tool in use had begun. A
    /// drawing tool becomes the one the Draw button holds
    /// ([`crate::Prefs::drawing_tool`]). While an operation's targets are
    /// being edited the drawing is theirs, and only Select can be armed.
    pub fn arm(&mut self, tool: Tool) {
        if self.target_editing().is_some() && tool != Tool::Select {
            return;
        }
        self.tool = Session {
            tool,
            draft: Draft::default(),
        };
        if let Tool::Draw(drawing) = tool {
            self.prefs_mut().drawing_tool = drawing;
        }
    }

    /// What the tool in use wants drawn.
    #[must_use]
    pub fn draft(&self) -> &Draft {
        &self.tool.draft
    }

    /// Hands `event` to the tool in use.
    pub fn tool_event(&mut self, event: Event) -> Reply {
        match (self.tool.tool, event) {
            (Tool::Select | Tool::Tab, _) => {}
            (_, Event::Move(pointer)) => {
                self.tool.draft.snap = Some(self.snap_at(&pointer, None, &[]));
            }
            (_, Event::Leave) => self.tool.draft.snap = None,
            (_, Event::Key(Key::Escape)) => self.arm(Tool::Select),
            _ => {}
        }
        Reply::default()
    }

    /// The snap grid's spacing at the Viewport's zoom, mm (capability
    /// 82a): the user's, or the automatic one.
    #[must_use]
    pub fn grid_step_mm(&self) -> f64 {
        snap::grid_step_mm(
            self.view().camera.px_per_mm,
            self.project().display_units,
            self.prefs().grid_spacing_mm,
        )
    }

    /// The point a tool takes for `pointer` (capability 82): snapped to
    /// the visible shapes, the stock, the origin and the grid, as the
    /// snapping preferences say. `reference` is the tool's last point;
    /// `skip` the shapes that are no target.
    #[must_use]
    pub fn snap_at(
        &self,
        pointer: &Pointer,
        reference: Option<Point>,
        skip: &[InstanceId],
    ) -> Snapped {
        let project = self.project();
        let targets = Targets {
            scene: self.scene(),
            stock: &project.stock,
            origin: project.origin.xy.point_mm(&project.stock),
            px_per_mm: self.view().camera.px_per_mm,
            // A hidden grid does not snap.
            grid_step_mm: self.view().visibility.grid.then(|| self.grid_step_mm()),
            skip,
        };
        snap::resolve(&targets, &self.prefs().snap, pointer, reference)
    }
}
