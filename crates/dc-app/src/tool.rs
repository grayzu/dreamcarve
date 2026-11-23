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

mod adjust;
mod arrays;
mod draw;
mod gear;
mod node;
mod path;
mod trim;

use std::collections::BTreeMap;

use dc_geometry::shapes::gear::{GearCrest, GearFlanks, GearRoot};
use dc_geometry::{Point, Polyline};
use dc_project::{InstanceId, Measured};

use crate::App;
use crate::snap::{self, Pointer, Snapped, Targets};

pub use adjust::AdjustPrefs;
pub use arrays::{ArrayKind, ArrayPrefs};
pub use draw::MIN_SIZE_MM;
pub(crate) use draw::{field_label, parse_angle_deg, polygon_name, size_text};
pub use node::{
    BarAction, Control, HANDLE_HIT_PX, HandleMark, JOIN_PX, NODE_HIT_PX, NodeAction, NodeMark,
    NodeRef,
};

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
    /// A Shape menu command that takes a value first, with the tool bar
    /// for its popover (capabilities 93, 94 and 97).
    Adjust(Adjust),
}

/// A Shape menu command that takes a value before it acts on the
/// selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Adjust {
    /// **Offset…**: a distance, a side, and whether the original stays.
    Offset,
    /// **Fillet Corners…**: a radius.
    FilletCorners,
    /// **Join**: a tolerance.
    Join,
    /// **Array…** (the Arrange menu's): a grid's counts and spacing, or
    /// a circular array's count, centre and angle.
    Array,
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

    /// The tool's name: the Draw popover's row, the Draw menu's, the
    /// tool bar's heading.
    #[must_use]
    pub const fn name(self) -> crate::StrKey {
        use crate::StrKey;
        match self {
            Self::Line => StrKey::DrawNameLine,
            Self::Arc => StrKey::DrawNameArc,
            Self::Spline => StrKey::DrawNameSpline,
            Self::Rectangle => StrKey::DrawNameRectangle,
            Self::Circle => StrKey::DrawNameCircle,
            Self::Ellipse => StrKey::DrawNameEllipse,
            Self::Polygon => StrKey::DrawNamePolygon,
            Self::Slot => StrKey::DrawNameSlot,
            Self::Gear => StrKey::DrawNameGear,
            Self::Text => StrKey::DrawNameText,
        }
    }

    /// Whether this build has the tool: the popover and the menu list
    /// the ones it has. Text arrives with its fonts.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        !matches!(self, Self::Text)
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

/// A size of the step in hand, which the tool bar shows as a field and
/// takes typed (capability 83).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SizeField {
    /// The point's X from the origin.
    X,
    /// The point's Y from the origin.
    Y,
    /// A rectangle's or an ellipse's width.
    Width,
    /// A rectangle's or an ellipse's height.
    Height,
    /// A circle's diameter.
    Diameter,
    /// A polygon's radius.
    Radius,
    /// A direction, degrees anticlockwise from +X.
    Angle,
    /// A slot's length, centre to centre.
    Length,
    /// A slot's width.
    SlotWidth,
    /// How many teeth a gear has.
    Teeth,
    /// A gear's pressure angle, degrees.
    PressureAngle,
    /// A gear's whole depth, a tooth's tip to its root.
    WholeDepth,
    /// The fillet of a gear's rounded root.
    RootFillet,
    /// The radius of a gear's rounded crest.
    CrestRadius,
    /// The diameter of a gear's bore; none for no bore.
    Bore,
    /// The size of a rectangle's rounded or chamfered corners.
    CornerSize,
    /// How many sides a polygon has.
    Sides,
    /// How far an offset is from its outline.
    Distance,
    /// How near two ends must be to be joined.
    Tolerance,
    /// How many columns a grid of copies has.
    Columns,
    /// How many rows a grid of copies has.
    Rows,
    /// A grid's spacing along X.
    SpacingX,
    /// A grid's spacing along Y.
    SpacingY,
    /// How many copies a circular array has, the source among them.
    Copies,
}

/// How a size's text is read and shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    /// A length: in project units, or with a unit of its own (`1/4 in`).
    Length,
    /// An angle, degrees.
    Angle,
    /// A whole number of things, with no unit.
    Count,
}

impl SizeField {
    /// Every size, in declaration order.
    pub const ALL: [Self; 24] = [
        Self::X,
        Self::Y,
        Self::Width,
        Self::Height,
        Self::Diameter,
        Self::Radius,
        Self::Angle,
        Self::Length,
        Self::SlotWidth,
        Self::Teeth,
        Self::PressureAngle,
        Self::WholeDepth,
        Self::RootFillet,
        Self::CrestRadius,
        Self::Bore,
        Self::CornerSize,
        Self::Sides,
        Self::Distance,
        Self::Tolerance,
        Self::Columns,
        Self::Rows,
        Self::SpacingX,
        Self::SpacingY,
        Self::Copies,
    ];

    /// The size's label in the tool bar.
    #[must_use]
    pub const fn label(self) -> crate::StrKey {
        draw::field_label(self)
    }

    /// What the size is: its label's tooltip.
    #[must_use]
    pub const fn help(self) -> crate::StrKey {
        draw::field_help(self)
    }

    /// Whether the size is a length or an angle.
    #[must_use]
    pub const fn quantity(self) -> Quantity {
        match self {
            Self::Angle | Self::PressureAngle => Quantity::Angle,
            Self::Teeth | Self::Sides | Self::Columns | Self::Rows | Self::Copies => {
                Quantity::Count
            }
            _ => Quantity::Length,
        }
    }

    /// Whether the size must be above zero. A coordinate and an angle
    /// may be anything.
    const fn must_be_positive(self) -> bool {
        !matches!(
            self,
            Self::X | Self::Y | Self::Angle | Self::SpacingX | Self::SpacingY
        )
    }
}

/// One field of the tool bar.
#[derive(Debug, Clone, PartialEq)]
pub struct SizeRow {
    /// Which size.
    pub field: SizeField,
    /// Its label: *W*, *Diameter*.
    pub label: String,
    /// What the size is, for the label's tooltip and the help line.
    pub help: String,
    /// Its text: the typed value while it is pinned, else what the
    /// pointer gives, in project units with no unit after it; empty
    /// while the pointer has given nothing.
    pub text: String,
    /// Length or angle, for the unit the field draws after the value.
    pub quantity: Quantity,
    /// Whether the value was typed: the preview follows the pointer only
    /// in what is not pinned.
    pub pinned: bool,
    /// Why the last text typed was refused.
    pub problem: Option<String>,
    /// Whether this is an option of the tool, kept for the shapes that
    /// follow (a rectangle's corner size, a polygon's sides), and no size
    /// of the step in hand: `Enter` in its field writes it and commits
    /// nothing.
    pub option: bool,
}

/// What the tool bar shows while a tool is armed (capability 81).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolBar {
    /// The tool.
    pub tool: Tool,
    /// What the next click does.
    pub prompt: String,
    /// The sizes of the step in hand.
    pub fields: Vec<SizeRow>,
    /// The tool's choices, each one of a few: a gear's forms.
    pub choices: Vec<Choice>,
    /// The tool's buttons: the Edit nodes tool's *Break*, and *Convert to
    /// Path* when the shape keeps its sizes.
    pub actions: Vec<BarAction>,
    /// A line under the fields that is read, not typed: a gear's module.
    pub note: Option<String>,
    /// Whether **Done** would make a shape now: a box tool's shape with
    /// the sizes as they read, a path with the points it has.
    pub can_finish: bool,
}

/// One of a tool's choices in the tool bar: a label and the few things
/// it can be.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// What is chosen: *Flanks*.
    pub label: String,
    /// What the choice is, for the label's tooltip and the help line.
    pub help: String,
    /// What it can be, in order.
    pub options: Vec<ChoiceOption>,
    /// Which of `options` it is.
    pub selected: usize,
}

/// One thing a [`Choice`] can be.
#[derive(Debug, Clone, PartialEq)]
pub struct ChoiceOption {
    /// Its name: *Involute*.
    pub label: String,
    /// The option that chooses it, for [`Event::Option`].
    pub option: ToolOption,
}

/// How a rectangle's corners are finished, as the Rectangle tool's
/// option has it; the size is its own option.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CornerStyle {
    /// Sharp.
    #[default]
    Square,
    /// Rounded by an arc.
    Rounded,
    /// Cut off straight.
    Chamfered,
}

impl CornerStyle {
    /// Every style, in the option's order.
    pub const ALL: [Self; 3] = [Self::Square, Self::Rounded, Self::Chamfered];

    /// The style's name in `gui.toml`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Square => "square",
            Self::Rounded => "rounded",
            Self::Chamfered => "chamfered",
        }
    }

    /// The style named `key` in `gui.toml`.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.key() == key)
    }
}

/// An option of a drawing tool, set in the tool bar and kept for the
/// shapes that follow (capabilities 85, 85b and 90).
#[derive(Debug, Clone, PartialEq)]
pub enum ToolOption {
    /// The Rectangle tool's corners.
    Corners(CornerStyle),
    /// The size of a rounded or chamfered corner, as typed.
    CornerSize(String),
    /// The Polygon tool's sides, 3 to 50.
    Sides(u32),
    /// What the Polygon tool's radius reaches.
    Measured(Measured),
    /// Whether the shapes that follow are construction geometry.
    Construction(bool),
    /// The form of the gear's flanks, while its sizes are set.
    Flanks(GearFlanks),
    /// The form of the gear's root.
    Root(GearRoot),
    /// The form of the gear's crest.
    Crest(GearCrest),
    /// Which side an offset is made on.
    OffsetSide(crate::edit::OffsetSide),
    /// Whether an offset's original stays.
    KeepOriginal(bool),
    /// Whether an array is a grid or circular.
    ArrayKind(ArrayKind),
    /// What a grid's spacings measure.
    ArraySpacing(crate::array::GridSpacing),
    /// Whether a circular array's copies are turned.
    ArrayTurn(bool),
}

/// What the window tells the tool.
#[derive(Debug, Clone, PartialEq)]
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
    /// A size typed in the tool bar and committed. Empty text unpins it.
    Field {
        /// Which size.
        field: SizeField,
        /// What was typed.
        text: String,
    },
    /// An option set in the tool bar.
    Option(ToolOption),
    /// The tool bar's **Done**: finish with the sizes as they read.
    Done,
    /// Something asked of the Edit nodes tool: a key, a menu's row, a
    /// button of its bar.
    Node(NodeAction),
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
    /// The nodes of the path being edited (capability 92).
    pub nodes: Vec<NodeMark>,
    /// Its curves' handles and its arcs' middles.
    pub handles: Vec<HandleMark>,
    /// The selected span of it, to draw over the outline.
    pub highlights: Vec<Polyline>,
    /// The box being dragged round nodes: its two corners, mm.
    pub marquee: Option<(Point, Point)>,
}

/// The tool in use and what it holds. Not project data.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Session {
    pub(crate) tool: Tool,
    pub(crate) draft: Draft,
    /// How far a drawing tool has got.
    state: draw::State,
    /// The path the Line, Arc or Spline tool is drawing.
    path: path::PathDraft,
    /// How far the Gear tool has got.
    gear: gear::GearDraft,
    /// What the Edit nodes tool has selected and hold of.
    node: node::NodeDraft,
    /// Where the pointer last was, with what was held.
    pointer: Option<Pointer>,
    /// Where the primary button went down, mm: a release far from it is
    /// a drag, and gives the next point.
    pressed_at: Option<Point>,
    /// The sizes typed for the step in hand: millimetres or degrees.
    pins: BTreeMap<SizeField, f64>,
    /// Why the last text typed in a field was refused.
    problems: BTreeMap<SizeField, String>,
    /// Whether the shapes that follow are construction geometry
    /// (capability 90): the tool bar's switch, for this session.
    pub(crate) construction: bool,
    /// The centre of the circular array being set, once it is typed or
    /// clicked (capability 98).
    array_centre: Option<Point>,
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
    ///
    /// Line, Arc and Spline are one path's three kinds of span
    /// (capability 88): arming one while another has a path begun keeps
    /// the path, and the spans that follow are the new tool's.
    pub fn arm(&mut self, tool: Tool) {
        if self.target_editing().is_some() && tool != Tool::Select {
            return;
        }
        let same_path = matches!(
            (self.tool.tool, tool),
            (Tool::Draw(from), Tool::Draw(to)) if path::draws(from) && path::draws(to)
        ) && self.tool.path.begun();
        if same_path {
            self.tool.tool = tool;
            self.tool.path.changed_tool();
            self.tool.pins.clear();
            self.tool.problems.clear();
            if let Tool::Draw(drawing) = tool {
                self.refresh_path_draft(drawing);
            }
        } else {
            self.tool = Session {
                tool,
                // The switch outlives a change of tool.
                construction: self.tool.construction,
                ..Session::default()
            };
        }
        if let Tool::Draw(drawing) = tool {
            self.prefs_mut().drawing_tool = drawing;
        }
        if tool == Tool::Nodes {
            self.begin_nodes();
        }
        if let Tool::Adjust(adjust) = tool {
            self.refresh_adjust_draft(adjust);
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
            (Tool::Draw(drawing), event) if draw::draws(drawing) => {
                return self.draw_event(drawing, event);
            }
            (Tool::Draw(drawing), event) if path::draws(drawing) => {
                return self.path_event(drawing, event);
            }
            (Tool::Draw(drawing), event) if gear::draws(drawing) => {
                return self.gear_event(event);
            }
            (Tool::Nodes, event) => return self.node_event(event),
            (Tool::Trim, event) => return self.trim_event(event),
            (Tool::Adjust(adjust), event) => return self.adjust_event(adjust, event),
            (_, Event::Move(pointer)) => {
                self.tool.draft.snap = Some(self.snap_at(&pointer, None, &[]));
            }
            (_, Event::Leave) => self.tool.draft.snap = None,
            (_, Event::Key(Key::Escape)) => self.arm(Tool::Select),
            _ => {}
        }
        Reply::default()
    }

    /// What the tool bar shows: `None` under Select and the Tab tool,
    /// which have none.
    #[must_use]
    pub fn tool_bar(&self) -> Option<ToolBar> {
        let mut bar = match self.tool.tool {
            Tool::Draw(drawing) if draw::draws(drawing) => self.draw_bar(drawing),
            Tool::Draw(drawing) if path::draws(drawing) => self.path_bar(drawing),
            Tool::Draw(drawing) if gear::draws(drawing) => self.gear_bar(),
            Tool::Nodes => self.node_bar(),
            Tool::Trim => self.trim_bar(),
            Tool::Adjust(adjust) => self.adjust_bar(adjust),
            _ => return None,
        };
        // Every drawing tool's last choice: whether the shapes that
        // follow are construction geometry (capability 100).
        if matches!(self.tool.tool, Tool::Draw(_)) {
            bar.choices.push(crate::shape::construction_choice(
                self.strings(),
                self.tool.construction,
            ));
        }
        Some(bar)
    }

    /// Whether the shapes drawn next are construction geometry.
    #[must_use]
    pub fn drawing_construction(&self) -> bool {
        self.tool.construction
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
