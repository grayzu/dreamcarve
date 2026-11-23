// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/slices/pendingAddSlice.ts
//   src/components/canvas/useClickPlacement.ts
//   src/components/canvas/manualEntry.ts
//   src/components/canvas/useCreationWorkflow.ts
//   src/store/helpers/buildShapeFeature.ts

//! The drawing tools that take a shape from two or three points
//! (`docs/specs/phase-5/design.md`, "The tool state machines"; functional
//! capabilities 83 to 85c and 90): Rectangle, Circle, Ellipse, Polygon
//! and Slot.
//!
//! Each is upstream's click sequence: a first point, then the point that
//! sizes the shape, and for a slot a third for its width. Every step can
//! be typed instead (`manualEntry.ts`): a typed size is pinned, and the
//! preview then follows the pointer only in what is not pinned.
//!
//! Changed from upstream:
//!
//! - A press, a drag and a release give two points, as two clicks do
//!   (Graphite's shape tool); upstream is click-click only.
//! - `Shift` makes a rectangle square and an ellipse a circle; `Alt`
//!   draws a rectangle from its centre (Graphite's `resize.rs`).
//! - The first point can be typed, as X and Y from the origin; upstream
//!   types sizes only.
//! - Every one of these is a parametric shape (ADR-0006), a rectangle
//!   and a circle too, which upstream bakes; each is centred on its own
//!   origin and placed by its instance's transform.
//! - A polygon can be measured to its flats.
//!
//! Re-derived for Y-up: an angle is degrees anticlockwise from +X, and a
//! typed +90° points up the Viewport, where upstream's points down its
//! screen. Nothing else here has a direction.

use dc_geometry::{Point, Polyline, Rect, Transform, Vec2};
use dc_project::{Corner, Measured, Shape, Units};

use super::{
    CornerStyle, Draft, Drawing, Event, Key, Label, Quantity, Reply, SizeField, SizeRow, ToolBar,
    ToolOption,
};
use crate::command::{NewShape, next_name};
use crate::snap::{Pointer, Snapped};
use crate::strings::{StrKey, Strings};
use crate::{App, Command, Tool, units};

/// The least a drawn shape can measure, mm (upstream's minimum size): a
/// shape under it on a side is not made.
pub const MIN_SIZE_MM: f64 = 0.01;

/// How far the pointer travels between a press and its release before
/// the two are a drag and not a click, logical pixels (phase 4's click
/// slop).
pub(super) const DRAG_PX: f64 = 4.0;

/// The fewest sides the Polygon tool takes.
const MIN_SIDES: u32 = dc_project::MIN_POLYGON_SIDES;
/// The most sides the Polygon tool takes.
const MAX_SIDES: u32 = dc_project::MAX_POLYGON_SIDES;

/// How far a drawing tool has got.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) enum State {
    /// Nothing begun: the next point is the first.
    #[default]
    Idle,
    /// The first point is taken: a corner, or a centre.
    First(Point),
    /// A slot's two end centres are taken: the next point sets its width.
    SlotWidth(Point, Point),
}

/// Whether `drawing` is one of this module's tools.
pub(super) const fn draws(drawing: Drawing) -> bool {
    matches!(
        drawing,
        Drawing::Rectangle | Drawing::Circle | Drawing::Ellipse | Drawing::Polygon | Drawing::Slot
    )
}

/// A shape as it would be made now.
struct Made {
    name: StrKey,
    shape: Shape,
    transform: Transform,
    /// The sizes the fields show, mm or degrees.
    sizes: Vec<(SizeField, f64)>,
    /// Where the size label goes, and what it says.
    label: Option<(Point, String)>,
}

impl App {
    /// Hands `event` to the drawing tool `drawing`.
    pub(super) fn draw_event(&mut self, drawing: Drawing, event: Event) -> Reply {
        match event {
            Event::Move(pointer) => {
                self.tool.pointer = Some(pointer);
                self.refresh_draft(drawing);
            }
            Event::Leave => {
                self.tool.pointer = None;
                self.refresh_draft(drawing);
            }
            Event::Press(pointer) => {
                self.tool.pointer = Some(pointer);
                self.tool.pressed_at = Some(pointer.at_mm);
                return self.take_point(drawing);
            }
            Event::Release(pointer) => {
                self.tool.pointer = Some(pointer);
                let dragged = self.tool.pressed_at.take().is_some_and(|pressed| {
                    pressed.distance_to(pointer.at_mm) * self.view().camera.px_per_mm > DRAG_PX
                });
                // A drag from the first point gives the second; a click's
                // release gives nothing more.
                if dragged && self.tool.state != State::Idle {
                    return self.take_point(drawing);
                }
                self.refresh_draft(drawing);
            }
            // A double-click is two clicks here; the Edit nodes tool's
            // actions are not this tool's.
            Event::DoubleClick(_) | Event::Node(_) => {}
            Event::Key(Key::Enter) | Event::Done => return self.take_point(drawing),
            Event::Key(Key::Escape) => {
                if self.tool.state == State::Idle {
                    self.arm(Tool::Select);
                } else {
                    self.restart(drawing);
                }
            }
            Event::Key(Key::Backspace | Key::Delete) => {
                // One step back: the width's step to the axis, the rest to
                // the start.
                self.tool.state = match self.tool.state {
                    State::SlotWidth(first, _) => State::First(first),
                    _ => State::Idle,
                };
                self.clear_step();
                self.refresh_draft(drawing);
            }
            Event::Field { field, text } => {
                self.type_size(field, &text);
                self.refresh_draft(drawing);
            }
            Event::Option(option) => {
                self.set_option(option);
                self.refresh_draft(drawing);
            }
        }
        Reply::default()
    }

    /// The tool bar for `drawing`: its prompt and the fields of the step.
    pub(super) fn draw_bar(&self, drawing: Drawing) -> ToolBar {
        let strings = self.strings();
        let units = self.project().display_units;
        let measured = self.prefs().drawing.polygon_measured;
        let prompt = match (drawing, self.tool.state) {
            (Drawing::Rectangle, State::Idle) => StrKey::DrawPromptRectangleFirst,
            (Drawing::Rectangle, _) => StrKey::DrawPromptRectangleSecond,
            (Drawing::Circle, State::Idle) => StrKey::DrawPromptCircleFirst,
            (Drawing::Circle, _) => StrKey::DrawPromptCircleSecond,
            (Drawing::Ellipse, State::Idle) => StrKey::DrawPromptEllipseFirst,
            (Drawing::Ellipse, _) => StrKey::DrawPromptEllipseSecond,
            (Drawing::Polygon, State::Idle) => StrKey::DrawPromptPolygonFirst,
            (Drawing::Polygon, _) if measured == Measured::Flats => {
                StrKey::DrawPromptPolygonSecondFlats
            }
            (Drawing::Polygon, _) => StrKey::DrawPromptPolygonSecond,
            (_, State::Idle) => StrKey::DrawPromptSlotFirst,
            (_, State::First(_)) => StrKey::DrawPromptSlotSecond,
            (_, State::SlotWidth(..)) => StrKey::DrawPromptSlotWidth,
        };
        let values = self.step_values(drawing);
        let mut fields: Vec<SizeRow> = step_fields(drawing, self.tool.state)
            .iter()
            .map(|&field| {
                let pinned = self.tool.pins.get(&field).copied();
                let value = pinned.or_else(|| {
                    values
                        .iter()
                        .find(|(shown, _)| *shown == field)
                        .map(|(_, value)| *value)
                });
                SizeRow {
                    field,
                    label: strings.get(field_label(field)).to_owned(),
                    help: strings.get(field.help()).to_owned(),
                    text: value.map_or_else(String::new, |value| size_text(field, value, units)),
                    quantity: field.quantity(),
                    pinned: pinned.is_some(),
                    problem: self.tool.problems.get(&field).cloned(),
                    option: false,
                }
            })
            .collect();
        // The tool's options, kept for the shapes that follow: after the
        // step's sizes, in every step.
        let drawing_prefs = self.prefs().drawing;
        let option = |field: SizeField, text: String| SizeRow {
            field,
            label: strings.get(field_label(field)).to_owned(),
            help: strings.get(field.help()).to_owned(),
            text,
            quantity: field.quantity(),
            pinned: false,
            problem: self.tool.problems.get(&field).cloned(),
            option: true,
        };
        let choices = match drawing {
            Drawing::Rectangle => {
                if drawing_prefs.corners != CornerStyle::Square {
                    fields.push(option(
                        SizeField::CornerSize,
                        units::field_text(drawing_prefs.corner_size_mm(units), units),
                    ));
                }
                vec![crate::shape::corners_choice(strings, drawing_prefs.corners)]
            }
            Drawing::Polygon => {
                fields.push(option(
                    SizeField::Sides,
                    drawing_prefs.polygon_sides.to_string(),
                ));
                vec![crate::shape::measure_choice(strings, measured)]
            }
            _ => Vec::new(),
        };
        ToolBar {
            tool: Tool::Draw(drawing),
            prompt: strings.get(prompt).to_owned(),
            fields,
            choices,
            actions: Vec::new(),
            note: None,
            can_finish: self.made(drawing).is_some(),
        }
    }

    /// Back to the tool's first step, with nothing typed.
    fn restart(&mut self, drawing: Drawing) {
        self.tool.state = State::Idle;
        self.tool.pressed_at = None;
        self.clear_step();
        self.refresh_draft(drawing);
    }

    /// Forgets what was typed for the step that is over.
    fn clear_step(&mut self) {
        self.tool.pins.clear();
        self.tool.problems.clear();
    }

    /// The pointer's point for the step in hand, snapped, with the point
    /// before it as the reference `Shift` and Perpendicular turn about.
    /// `None` while the pointer is off the Viewport.
    fn step_point(&self) -> Option<Snapped> {
        let pointer = self.tool.pointer?;
        let reference = match self.tool.state {
            State::Idle => None,
            State::First(first) => Some(first),
            // The width is a distance from the axis: nothing to turn about.
            State::SlotWidth(..) => None,
        };
        // On a box tool `Shift` squares the box; it does not hold the
        // corner to an angle.
        let boxed = matches!(
            self.tool.tool,
            Tool::Draw(Drawing::Rectangle | Drawing::Ellipse)
        );
        let pointer = Pointer {
            shift: pointer.shift && !boxed,
            ..pointer
        };
        Some(self.snap_at(&pointer, reference, &[]))
    }

    /// The first point as the step would take it: the pointer's, with a
    /// typed X or Y in its place. `None` while neither the pointer nor
    /// both fields give one.
    fn first_point(&self) -> Option<Point> {
        let origin = self.origin_mm();
        let pinned = |field: SizeField| self.tool.pins.get(&field).copied();
        let pointer = self.step_point().map(|snapped| snapped.at_mm);
        let x = pinned(SizeField::X)
            .map(|x| origin.x + x)
            .or(pointer.map(|p| p.x))?;
        let y = pinned(SizeField::Y)
            .map(|y| origin.y + y)
            .or(pointer.map(|p| p.y))?;
        Some(Point::new(x, y))
    }

    /// The work zero in project space, which typed coordinates are from.
    fn origin_mm(&self) -> Point {
        let project = self.project();
        project.origin.xy.point_mm(&project.stock)
    }

    /// The shape the step in hand would make now, when it would make one.
    fn made(&self, drawing: Drawing) -> Option<Made> {
        let pointer = self.step_point().map(|snapped| snapped.at_mm);
        let held = self.tool.pointer.unwrap_or_default();
        let pins = &self.tool.pins;
        let pin = |field: SizeField| pins.get(&field).copied();
        let drawing_prefs = self.prefs().drawing;
        let units = self.project().display_units;
        let strings = self.strings();
        match (drawing, self.tool.state) {
            (_, State::Idle) => None,
            (Drawing::Rectangle, State::First(first)) => {
                let (dx, dy) = box_offsets(first, pointer, held.shift);
                let (w_pin, h_pin) = (pin(SizeField::Width), pin(SizeField::Height));
                // From the centre, half of each size lies either side.
                let span = if held.alt { 2.0 } else { 1.0 };
                let width = w_pin.unwrap_or(dx.abs() * span);
                let height = h_pin.unwrap_or(dy.abs() * span);
                let centre = if held.alt {
                    first
                } else {
                    first + Vec2::new(side(dx) * width / 2.0, side(dy) * height / 2.0)
                };
                let size = drawing_prefs
                    .corner_size_mm(units)
                    .min(width.min(height) / 2.0);
                let corner = match drawing_prefs.corners {
                    CornerStyle::Square => Corner::Square,
                    CornerStyle::Rounded => Corner::Rounded { radius_mm: size },
                    CornerStyle::Chamfered => Corner::Chamfered { distance_mm: size },
                };
                Some(Made {
                    name: StrKey::DrawNameRectangle,
                    shape: Shape::Rectangle {
                        width_mm: width,
                        height_mm: height,
                        corner,
                    },
                    transform: Transform::translate(centre.x, centre.y),
                    sizes: vec![(SizeField::Width, width), (SizeField::Height, height)],
                    label: Some((
                        Point::new(centre.x, centre.y - height / 2.0),
                        box_label(strings, units, width, height),
                    )),
                })
            }
            (Drawing::Circle, State::First(centre)) => {
                let diameter = pin(SizeField::Diameter)
                    .or_else(|| pointer.map(|p| 2.0 * centre.distance_to(p)))?;
                Some(Made {
                    name: StrKey::DrawNameCircle,
                    shape: Shape::Circle {
                        diameter_mm: diameter,
                    },
                    transform: Transform::translate(centre.x, centre.y),
                    sizes: vec![(SizeField::Diameter, diameter)],
                    label: Some((
                        Point::new(centre.x, centre.y - diameter / 2.0),
                        strings.fmt(
                            StrKey::DrawLabelDiameter,
                            &[("diameter", &units::summary_text(diameter, units))],
                        ),
                    )),
                })
            }
            (Drawing::Ellipse, State::First(centre)) => {
                let (dx, dy) = box_offsets(centre, pointer, held.shift);
                let width = pin(SizeField::Width).unwrap_or(2.0 * dx.abs());
                let height = pin(SizeField::Height).unwrap_or(2.0 * dy.abs());
                Some(Made {
                    name: StrKey::DrawNameEllipse,
                    shape: Shape::Ellipse {
                        width_mm: width,
                        height_mm: height,
                    },
                    transform: Transform::translate(centre.x, centre.y),
                    sizes: vec![(SizeField::Width, width), (SizeField::Height, height)],
                    label: Some((
                        Point::new(centre.x, centre.y - height / 2.0),
                        box_label(strings, units, width, height),
                    )),
                })
            }
            (Drawing::Polygon, State::First(centre)) => {
                let offset = pointer.map(|p| p - centre);
                let radius = pin(SizeField::Radius).or(offset.map(Vec2::length))?;
                let angle_deg = pin(SizeField::Angle)
                    .or(offset
                        .filter(|o| o.length() > 0.0)
                        .map(|o| o.y.atan2(o.x).to_degrees()))
                    .unwrap_or(0.0);
                let sides = drawing_prefs.polygon_sides.clamp(MIN_SIDES, MAX_SIDES);
                let measured = drawing_prefs.polygon_measured;
                // To the flats, the pointer is at the middle of a side, half
                // a side's turn from the first corner.
                let turn = match measured {
                    Measured::Corners => 0.0,
                    Measured::Flats => std::f64::consts::PI / f64::from(sides),
                };
                Some(Made {
                    name: polygon_name(sides),
                    shape: Shape::Polygon {
                        sides,
                        radius_mm: radius,
                        measured,
                    },
                    transform: Transform::rotate(angle_deg.to_radians() + turn)
                        .then(Transform::translate(centre.x, centre.y)),
                    sizes: vec![(SizeField::Radius, radius), (SizeField::Angle, angle_deg)],
                    label: Some((
                        centre,
                        strings.fmt(
                            StrKey::DrawLabelRadius,
                            &[("radius", &units::summary_text(radius, units))],
                        ),
                    )),
                })
            }
            (Drawing::Slot, State::First(first)) => {
                let (length, angle_deg) = self.slot_axis(first, pointer)?;
                let width = pin(SizeField::SlotWidth);
                let second = first + direction(angle_deg) * length;
                Some(slot(strings, units, first, second, width))
            }
            (Drawing::Slot, State::SlotWidth(first, second)) => {
                let axis = second - first;
                let across = |p: Point| 2.0 * (p - first).cross(axis).abs() / axis.length();
                let width = pin(SizeField::SlotWidth).or(pointer.map(across))?;
                Some(slot(strings, units, first, second, Some(width)))
            }
            // The other tools have their own modules.
            (_, State::First(_) | State::SlotWidth(..)) => None,
        }
    }

    /// A slot's length and direction from its first centre: typed, or the
    /// pointer's.
    fn slot_axis(&self, first: Point, pointer: Option<Point>) -> Option<(f64, f64)> {
        let offset = pointer.map(|p| p - first);
        let length = self
            .tool
            .pins
            .get(&SizeField::Length)
            .copied()
            .or(offset.map(Vec2::length))?;
        let angle_deg = self
            .tool
            .pins
            .get(&SizeField::Angle)
            .copied()
            .or(offset
                .filter(|o| o.length() > 0.0)
                .map(|o| o.y.atan2(o.x).to_degrees()))
            .unwrap_or(0.0);
        Some((length, angle_deg))
    }

    /// What the fields of the step in hand read: the first point from the
    /// origin, or the sizes of the shape as it would be made.
    fn step_values(&self, drawing: Drawing) -> Vec<(SizeField, f64)> {
        if self.tool.state == State::Idle {
            let origin = self.origin_mm();
            return self
                .step_point()
                .map(|snapped| {
                    vec![
                        (SizeField::X, snapped.at_mm.x - origin.x),
                        (SizeField::Y, snapped.at_mm.y - origin.y),
                    ]
                })
                .unwrap_or_default();
        }
        self.made(drawing)
            .map(|made| made.sizes)
            .unwrap_or_default()
    }

    /// Takes the step's point: the first point, the slot's second centre,
    /// or the point that finishes the shape.
    fn take_point(&mut self, drawing: Drawing) -> Reply {
        let reply = match self.tool.state {
            State::Idle => {
                if let Some(first) = self.first_point() {
                    self.tool.state = State::First(first);
                    self.clear_step();
                }
                Reply::default()
            }
            State::First(first) if drawing == Drawing::Slot => {
                let pointer = self.step_point().map(|snapped| snapped.at_mm);
                let width = self.tool.pins.get(&SizeField::SlotWidth).copied();
                match self.slot_axis(first, pointer) {
                    // No axis: a slot cannot be that short.
                    Some((length, _)) if length < MIN_SIZE_MM => Reply::default(),
                    // The width typed with the length: the slot is whole.
                    Some(_) if width.is_some() => self.commit_made(drawing),
                    Some((length, angle_deg)) => {
                        let second = first + direction(angle_deg) * length;
                        self.tool.state = State::SlotWidth(first, second);
                        self.clear_step();
                        Reply::default()
                    }
                    None => Reply::default(),
                }
            }
            State::First(_) | State::SlotWidth(..) => self.commit_made(drawing),
        };
        self.refresh_draft(drawing);
        reply
    }

    /// Adds the shape the step would make, when it is one, and starts the
    /// tool again for the next (upstream's sticky tools).
    fn commit_made(&mut self, drawing: Drawing) -> Reply {
        let Some(made) = self.made(drawing) else {
            return Reply::default();
        };
        let tolerance_mm = self.project().flatten_tolerance_mm;
        let Some(Ok(profiles)) = made.shape.outline(tolerance_mm) else {
            return Reply::default();
        };
        // A shape too small to be one is not made, and the tool stays as
        // it was (capability 90).
        let large_enough = profiles
            .first()
            .and_then(Polyline::bounds)
            .is_some_and(|b| b.width_mm() >= MIN_SIZE_MM && b.height_mm() >= MIN_SIZE_MM);
        if !large_enough {
            return Reply::default();
        }
        let name = next_name(self.project(), self.strings().get(made.name));
        let command = Command::AddShapes(vec![NewShape {
            name,
            shape: Some(made.shape),
            profiles,
            transform: made.transform,
            construction: self.tool.construction,
            folder: self.selection_folder(),
        }]);
        match self.run(command) {
            Ok(label) => {
                self.restart(drawing);
                Reply {
                    ran: Some(label),
                    refused: None,
                }
            }
            Err(error) => Reply {
                ran: None,
                refused: Some(error.to_string()),
            },
        }
    }

    /// The folder a new shape goes in: the one that holds the selection,
    /// when the whole selection is in one (capability 90).
    pub(super) fn selection_folder(&self) -> Option<dc_project::FolderId> {
        let selected = self.selection().instances();
        let mut folders = selected.iter().map(|id| {
            self.project()
                .instances
                .iter()
                .find(|instance| &instance.id == id)
                .and_then(|instance| instance.folder.clone())
        });
        let first = folders.next()??;
        folders
            .all(|folder| folder.as_ref() == Some(&first))
            .then_some(first)
    }

    /// What `text` typed in `field` is, millimetres, degrees or a count;
    /// else why it is none of them.
    pub(super) fn read_size(&self, field: SizeField, text: &str) -> Result<f64, String> {
        crate::shape::read_size(self.strings(), self.project().display_units, field, text)
    }

    /// Reads a typed size: pins it, or says at its field why not. Empty
    /// text unpins it.
    pub(super) fn type_size(&mut self, field: SizeField, text: &str) {
        self.tool.problems.remove(&field);
        if text.trim().is_empty() {
            self.tool.pins.remove(&field);
            return;
        }
        // A tool's option is no size of the step: it is kept for the
        // shapes that follow, and pins nothing.
        if matches!(field, SizeField::CornerSize | SizeField::Sides) {
            let problem = match (field, self.read_size(field, text)) {
                (_, Err(problem)) => Some(problem),
                (SizeField::CornerSize, Ok(size)) if size > 0.0 => {
                    self.prefs_mut().drawing.corner_mm = Some(size);
                    None
                }
                (SizeField::CornerSize, Ok(_)) => Some(
                    self.strings()
                        .get(StrKey::DrawProblemNotPositive)
                        .to_owned(),
                ),
                (_, Ok(sides))
                    if (f64::from(MIN_SIDES)..=f64::from(MAX_SIDES)).contains(&sides) =>
                {
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a whole number between 3 and 50"
                    )]
                    let sides = sides as u32;
                    self.prefs_mut().drawing.polygon_sides = sides;
                    None
                }
                (_, Ok(_)) => Some(self.strings().fmt(
                    StrKey::ShapeProblemSides,
                    &[
                        ("min", &MIN_SIDES.to_string()),
                        ("max", &MAX_SIDES.to_string()),
                    ],
                )),
            };
            if let Some(problem) = problem {
                self.tool.problems.insert(field, problem);
            }
            return;
        }
        let value = self.read_size(field, text).and_then(|value| {
            if field.must_be_positive() && value <= 0.0 {
                Err(self
                    .strings()
                    .get(StrKey::DrawProblemNotPositive)
                    .to_owned())
            } else {
                Ok(value)
            }
        });
        match value {
            Ok(value) => {
                self.tool.pins.insert(field, value);
            }
            Err(problem) => {
                self.tool.problems.insert(field, problem);
            }
        }
    }

    /// Sets a tool's option, kept for the shapes that follow.
    pub(super) fn set_option(&mut self, option: ToolOption) {
        let units = self.project().display_units;
        match option {
            ToolOption::Corners(corners) => self.prefs_mut().drawing.corners = corners,
            ToolOption::CornerSize(text) => {
                // A size that is no length leaves the size as it was.
                if let Ok(size) = units::parse_length_mm(&text, units)
                    && size > 0.0
                {
                    self.prefs_mut().drawing.corner_mm = Some(size);
                }
            }
            ToolOption::Sides(sides) => {
                self.prefs_mut().drawing.polygon_sides = sides.clamp(MIN_SIDES, MAX_SIDES);
            }
            ToolOption::Measured(measured) => self.prefs_mut().drawing.polygon_measured = measured,
            ToolOption::Construction(construction) => self.tool.construction = construction,
            // The Gear tool's, which holds them with the gear being set.
            ToolOption::Flanks(_) | ToolOption::Root(_) | ToolOption::Crest(_) => {}
        }
    }

    /// What the tool wants drawn: the shape as it would be made, where
    /// the next point would be taken, and the shape's sizes.
    fn refresh_draft(&mut self, drawing: Drawing) {
        let snap = self.step_point();
        let made = self.made(drawing);
        let tolerance_mm = self.project().flatten_tolerance_mm;
        let mut draft = Draft {
            snap,
            ..Draft::default()
        };
        // A slot's axis, before it has a width: its centreline.
        if let (Drawing::Slot, State::First(first), None) = (
            drawing,
            self.tool.state,
            self.tool.pins.get(&SizeField::SlotWidth),
        ) && let Some((length, angle_deg)) =
            self.slot_axis(first, snap.map(|snapped| snapped.at_mm))
        {
            let second = first + direction(angle_deg) * length;
            draft.outlines.push(Polyline::new(
                vec![
                    dc_geometry::Vertex::line(first.x, first.y),
                    dc_geometry::Vertex::line(second.x, second.y),
                ],
                false,
            ));
            draft.labels.push(Label {
                at_mm: first + (second - first) * 0.5,
                text: self.strings().fmt(
                    StrKey::DrawLabelLength,
                    &[(
                        "length",
                        &units::summary_text(length, self.project().display_units),
                    )],
                ),
            });
        } else if let Some(made) = made
            && let Some(Ok(profiles)) = made.shape.outline(tolerance_mm)
        {
            draft.outlines = profiles
                .iter()
                .filter_map(|profile| made.transform.apply_polyline(profile).ok())
                .collect();
            if let Some((at_mm, text)) = made.label {
                draft.labels.push(Label { at_mm, text });
            }
        }
        self.tool.draft = draft;
    }
}

/// The fields of each tool's steps, in `Tab` order (capability 83).
fn step_fields(drawing: Drawing, state: State) -> &'static [SizeField] {
    match (drawing, state) {
        (_, State::Idle) => &[SizeField::X, SizeField::Y],
        (Drawing::Rectangle | Drawing::Ellipse, _) => &[SizeField::Width, SizeField::Height],
        (Drawing::Circle, _) => &[SizeField::Diameter],
        (Drawing::Polygon, _) => &[SizeField::Radius, SizeField::Angle],
        (_, State::First(_)) => &[SizeField::Length, SizeField::Angle, SizeField::SlotWidth],
        (_, State::SlotWidth(..)) => &[SizeField::SlotWidth],
    }
}

pub(crate) const fn field_label(field: SizeField) -> StrKey {
    match field {
        SizeField::X => StrKey::DrawFieldX,
        SizeField::Y => StrKey::DrawFieldY,
        SizeField::Width => StrKey::DrawFieldWidth,
        SizeField::Height => StrKey::DrawFieldHeight,
        SizeField::Diameter => StrKey::DrawFieldDiameter,
        SizeField::Radius => StrKey::DrawFieldRadius,
        SizeField::Angle => StrKey::DrawFieldAngle,
        SizeField::Length => StrKey::DrawFieldLength,
        SizeField::SlotWidth => StrKey::DrawFieldSlotWidth,
        SizeField::Teeth => StrKey::DrawFieldTeeth,
        SizeField::PressureAngle => StrKey::DrawFieldPressureAngle,
        SizeField::WholeDepth => StrKey::DrawFieldWholeDepth,
        SizeField::RootFillet => StrKey::DrawFieldRootFillet,
        SizeField::CrestRadius => StrKey::DrawFieldCrestRadius,
        SizeField::Bore => StrKey::DrawFieldBore,
        SizeField::CornerSize => StrKey::DrawFieldCornerSize,
        SizeField::Sides => StrKey::DrawFieldSides,
    }
}

pub(crate) const fn field_help(field: SizeField) -> StrKey {
    match field {
        SizeField::X => StrKey::DrawHelpX,
        SizeField::Y => StrKey::DrawHelpY,
        SizeField::Width => StrKey::DrawHelpWidth,
        SizeField::Height => StrKey::DrawHelpHeight,
        SizeField::Diameter => StrKey::DrawHelpDiameter,
        SizeField::Radius => StrKey::DrawHelpRadius,
        SizeField::Angle => StrKey::DrawHelpAngle,
        SizeField::Length => StrKey::DrawHelpLength,
        SizeField::SlotWidth => StrKey::DrawHelpSlotWidth,
        SizeField::Teeth => StrKey::DrawHelpTeeth,
        SizeField::PressureAngle => StrKey::DrawHelpPressureAngle,
        SizeField::WholeDepth => StrKey::DrawHelpWholeDepth,
        SizeField::RootFillet => StrKey::DrawHelpRootFillet,
        SizeField::CrestRadius => StrKey::DrawHelpCrestRadius,
        SizeField::Bore => StrKey::DrawHelpBore,
        SizeField::CornerSize => StrKey::DrawHelpCornerSize,
        SizeField::Sides => StrKey::DrawHelpSides,
    }
}

/// What a polygon of `sides` sides is called (upstream names 3 to 12).
pub(crate) const fn polygon_name(sides: u32) -> StrKey {
    match sides {
        3 => StrKey::DrawNamePolygon3,
        4 => StrKey::DrawNamePolygon4,
        5 => StrKey::DrawNamePolygon5,
        6 => StrKey::DrawNamePolygon6,
        7 => StrKey::DrawNamePolygon7,
        8 => StrKey::DrawNamePolygon8,
        9 => StrKey::DrawNamePolygon9,
        10 => StrKey::DrawNamePolygon10,
        11 => StrKey::DrawNamePolygon11,
        12 => StrKey::DrawNamePolygon12,
        _ => StrKey::DrawNamePolygon,
    }
}

/// From `first` to the pointer on each axis; with `square`, the longer of
/// the two on both, each keeping its side. Nothing while the pointer is
/// off the Viewport.
fn box_offsets(first: Point, pointer: Option<Point>, square: bool) -> (f64, f64) {
    let offset = pointer.map_or(Vec2::new(0.0, 0.0), |p| p - first);
    if !square {
        return (offset.x, offset.y);
    }
    let longer = offset.x.abs().max(offset.y.abs());
    (side(offset.x) * longer, side(offset.y) * longer)
}

/// Which side of the first point an offset is on: −1 below zero, else +1,
/// so a size typed before the pointer has moved grows up and to the
/// right (`signX`, `signY`).
fn side(offset: f64) -> f64 {
    if offset < 0.0 { -1.0 } else { 1.0 }
}

/// The unit direction `angle_deg` anticlockwise from +X.
fn direction(angle_deg: f64) -> Vec2 {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    Vec2::new(cos, sin)
}

/// The slot between two end centres: its centreline's length and angle,
/// and `width_mm` when it has one yet (a slot with none is shown by its
/// centreline, and made with the least width there is).
fn slot(
    strings: &Strings,
    units: Units,
    first: Point,
    second: Point,
    width_mm: Option<f64>,
) -> Made {
    let axis = second - first;
    let length = axis.length();
    let angle_rad = axis.y.atan2(axis.x);
    let middle = first + axis * 0.5;
    let width = width_mm.unwrap_or(0.0);
    let bounds = Rect::from_point(first).including(second);
    Made {
        name: StrKey::DrawNameSlot,
        shape: Shape::Slot {
            length_mm: length,
            width_mm: width,
        },
        transform: Transform::rotate(angle_rad).then(Transform::translate(middle.x, middle.y)),
        sizes: vec![
            (SizeField::Length, length),
            (SizeField::Angle, angle_rad.to_degrees()),
            (SizeField::SlotWidth, width),
        ],
        label: Some((
            Point::new(middle.x, bounds.min.y - width / 2.0),
            box_label(strings, units, length, width),
        )),
    }
}

/// `60 × 30`, in project units.
fn box_label(strings: &Strings, units: Units, width_mm: f64, height_mm: f64) -> String {
    strings.fmt(
        StrKey::DrawLabelBox,
        &[
            ("width", &units::summary_text(width_mm, units)),
            ("height", &units::summary_text(height_mm, units)),
        ],
    )
}

/// A size as its field's text: a length in `units`, an angle in
/// degrees, a count as it is.
pub(crate) fn size_text(field: SizeField, value: f64, units: Units) -> String {
    match field.quantity() {
        Quantity::Length => units::field_text(value, units),
        Quantity::Angle => angle_text(value),
        Quantity::Count => format!("{value:.0}"),
    }
}

/// An angle as a field's text: degrees to two places, trailing zeros
/// dropped, negative zero as zero (`formatAngleField`).
pub(super) fn angle_text(angle_deg: f64) -> String {
    let text = format!("{angle_deg:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

/// A typed angle, degrees: a number, with `°` or `deg` after it or not.
pub(crate) fn parse_angle_deg(text: &str) -> Option<f64> {
    let text = text.trim();
    let number = text
        .strip_suffix('°')
        .or_else(|| text.strip_suffix("deg"))
        .unwrap_or(text)
        .trim();
    number.parse::<f64>().ok().filter(|angle| angle.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_read_and_show_as_degrees() {
        assert_eq!(parse_angle_deg("30"), Some(30.0));
        assert_eq!(parse_angle_deg(" -45.5° "), Some(-45.5));
        assert_eq!(parse_angle_deg("90 deg"), Some(90.0));
        assert_eq!(parse_angle_deg("ninety"), None);
        assert_eq!(parse_angle_deg("inf"), None);
        assert_eq!(parse_angle_deg(""), None);
        assert_eq!(angle_text(30.0), "30");
        assert_eq!(angle_text(-45.5), "-45.5");
        assert_eq!(angle_text(7.594_6), "7.59");
        assert_eq!(angle_text(-0.001), "0");
    }

    #[test]
    fn a_size_typed_before_the_pointer_moves_grows_up_and_right() {
        assert_eq!(side(0.0).to_bits(), 1.0f64.to_bits());
        assert_eq!(side(-0.5).to_bits(), (-1.0f64).to_bits());
        let first = Point::new(10.0, 10.0);
        assert_eq!(box_offsets(first, None, false), (0.0, 0.0));
        assert_eq!(
            box_offsets(first, Some(Point::new(40.0, -5.0)), false),
            (30.0, -15.0)
        );
        // Squared: the longer side on both axes, each on its own side.
        assert_eq!(
            box_offsets(first, Some(Point::new(40.0, -5.0)), true),
            (30.0, -30.0)
        );
    }
}
