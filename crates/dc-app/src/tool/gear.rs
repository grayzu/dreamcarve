// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/slices/pendingAddSlice.ts (setPendingGearRadiusAt,
//     setPendingGearParams, completePendingGear)
//   src/components/canvas/useClickPlacement.ts

//! The Gear tool (`docs/specs/phase-5/design.md`, "The tool state
//! machines"; functional capability 86): a click at the centre, one for
//! the outside radius, then the gear's sizes in the tool bar, previewed,
//! until **Done** makes it.
//!
//! The first radius gives the other sizes upstream's defaults for it
//! (`Gear::with_defaults`); after that each size is its own, and a new
//! tooth count or diameter leaves the rest as they read. A size is held
//! as it was typed, and what is wrong with the gear is said at the field
//! of the size at fault; the gear is not made while anything is.
//!
//! Changed from upstream: a radius under the least size is not taken,
//! where upstream's is raised to it; the limits in a reason are in
//! project units.

use dc_geometry::shapes::gear::{Gear, GearCrest, GearFlanks, GearProblem, GearRoot, GearSize};
use dc_geometry::{Point, Polyline, Transform, shapes};
use dc_project::Shape;

use super::draw::{DRAG_PX, MIN_SIZE_MM, angle_text, field_label};
use super::{
    Choice, ChoiceOption, Draft, Drawing, Event, Key, Label, Reply, SizeField, SizeRow, ToolBar,
    ToolOption,
};
use crate::command::{NewShape, next_name};
use crate::snap::Snapped;
use crate::strings::StrKey;
use crate::{App, Command, Tool, units};

/// How far the Gear tool has got.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) enum GearDraft {
    /// Nothing begun: the next point is the centre.
    #[default]
    Idle,
    /// The centre is taken: the next point is on the outside circle.
    Centre(Point),
    /// The centre and the outside radius are taken: the sizes are set in
    /// the tool bar.
    Sizes {
        /// The gear's centre, mm.
        centre: Point,
        /// The gear as the bar reads.
        gear: Gear,
    },
}

/// Whether `drawing` is this module's tool.
pub(super) const fn draws(drawing: Drawing) -> bool {
    matches!(drawing, Drawing::Gear)
}

/// The fields of the sizes step, in the bar's order; a size shows only
/// while the gear's form reads it.
fn size_fields(gear: &Gear) -> Vec<SizeField> {
    let mut fields = vec![SizeField::Teeth, SizeField::Diameter];
    if gear.flanks == GearFlanks::Involute {
        fields.push(SizeField::PressureAngle);
    }
    fields.push(SizeField::WholeDepth);
    if gear.root == GearRoot::Rounded {
        fields.push(SizeField::RootFillet);
    }
    if gear.crest == GearCrest::Rounded {
        fields.push(SizeField::CrestRadius);
    }
    fields.push(SizeField::Bore);
    fields
}

/// The field a gear's problem is said at.
const fn field_of(size: GearSize) -> SizeField {
    match size {
        GearSize::Outside => SizeField::Diameter,
        GearSize::Teeth => SizeField::Teeth,
        GearSize::WholeDepth => SizeField::WholeDepth,
        GearSize::PressureAngle => SizeField::PressureAngle,
        GearSize::RootFillet => SizeField::RootFillet,
        GearSize::CrestRadius => SizeField::CrestRadius,
        GearSize::Bore => SizeField::Bore,
    }
}

impl App {
    /// Hands `event` to the Gear tool.
    pub(super) fn gear_event(&mut self, event: Event) -> Reply {
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
                self.tool.pressed_at = Some(pointer.at_mm);
                self.gear_point();
                Reply::default()
            }
            Event::Release(pointer) => {
                self.tool.pointer = Some(pointer);
                let dragged = self.tool.pressed_at.take().is_some_and(|pressed| {
                    pressed.distance_to(pointer.at_mm) * self.view().camera.px_per_mm > DRAG_PX
                });
                // A drag from the centre gives the radius.
                if dragged && matches!(self.tool.gear, GearDraft::Centre(_)) {
                    self.gear_point();
                }
                Reply::default()
            }
            Event::DoubleClick(_) => Reply::default(),
            Event::Key(Key::Enter) | Event::Done => match self.tool.gear {
                GearDraft::Sizes { centre, gear } => self.make_gear(centre, &gear),
                _ => {
                    self.gear_point();
                    Reply::default()
                }
            },
            Event::Key(Key::Escape) => {
                if self.tool.gear == GearDraft::Idle {
                    self.arm(Tool::Select);
                    return Reply::default();
                }
                self.restart_gear(GearDraft::Idle);
                Reply::default()
            }
            Event::Key(Key::Backspace | Key::Delete) => {
                let back = match self.tool.gear {
                    GearDraft::Sizes { centre, .. } => GearDraft::Centre(centre),
                    _ => GearDraft::Idle,
                };
                self.restart_gear(back);
                Reply::default()
            }
            Event::Field { field, text } => {
                if matches!(self.tool.gear, GearDraft::Sizes { .. }) {
                    self.type_gear_size(field, &text);
                } else {
                    self.type_size(field, &text);
                }
                Reply::default()
            }
            Event::Option(option) => {
                if let GearDraft::Sizes { gear, .. } = &mut self.tool.gear {
                    match option {
                        ToolOption::Flanks(flanks) => gear.flanks = flanks,
                        ToolOption::Root(root) => gear.root = root,
                        ToolOption::Crest(crest) => gear.crest = crest,
                        _ => {}
                    }
                }
                self.set_option(option);
                Reply::default()
            }
        };
        self.refresh_gear_draft();
        reply
    }

    /// The tool bar for the Gear tool.
    pub(super) fn gear_bar(&self) -> ToolBar {
        let strings = self.strings();
        let units = self.project().display_units;
        let tool = Tool::Draw(Drawing::Gear);
        let GearDraft::Sizes { gear, .. } = self.tool.gear else {
            // The centre, then the radius: fields the pointer fills and a
            // typed value pins, as the other tools' are.
            let (prompt, shown): (_, &[SizeField]) = match self.tool.gear {
                GearDraft::Idle => (StrKey::DrawPromptGearFirst, &[SizeField::X, SizeField::Y]),
                _ => (StrKey::DrawPromptGearSecond, &[SizeField::Diameter]),
            };
            let values = self.gear_step_values();
            let fields = shown
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
                        label: strings.get(gear_label(field)).to_owned(),
                        text: value.map_or_else(String::new, |v| units::field_text(v, units)),
                        quantity: field.quantity(),
                        pinned: pinned.is_some(),
                        problem: self.tool.problems.get(&field).cloned(),
                    }
                })
                .collect();
            return ToolBar {
                tool,
                prompt: strings.get(prompt).to_owned(),
                fields,
                choices: Vec::new(),
                note: None,
                can_finish: false,
            };
        };
        let problems = gear.problems();
        let fields = size_fields(&gear)
            .into_iter()
            .map(|field| SizeRow {
                field,
                label: strings.get(gear_label(field)).to_owned(),
                text: match field {
                    SizeField::Teeth => gear.teeth.to_string(),
                    SizeField::Diameter => units::field_text(gear.outside_radius_mm * 2.0, units),
                    SizeField::PressureAngle => angle_text(gear.pressure_angle_deg),
                    SizeField::WholeDepth => units::field_text(gear.whole_depth_mm, units),
                    SizeField::RootFillet => units::field_text(gear.root_fillet_mm, units),
                    SizeField::CrestRadius => units::field_text(gear.crest_radius_mm, units),
                    _ => gear
                        .bore_diameter_mm
                        .map_or_else(String::new, |bore| units::field_text(bore, units)),
                },
                quantity: field.quantity(),
                // Nothing here follows the pointer.
                pinned: false,
                // Text that was no number first, then what the gear
                // cannot be.
                problem: self.tool.problems.get(&field).cloned().or_else(|| {
                    problems
                        .iter()
                        .find(|problem| field_of(problem.size()) == field)
                        .map(|problem| self.gear_problem_text(problem))
                }),
            })
            .collect();
        let choice = |label: StrKey, options: Vec<(StrKey, ToolOption)>, selected: ToolOption| {
            let selected = options
                .iter()
                .position(|(_, option)| *option == selected)
                .unwrap_or(0);
            Choice {
                label: strings.get(label).to_owned(),
                options: options
                    .into_iter()
                    .map(|(label, option)| ChoiceOption {
                        label: strings.get(label).to_owned(),
                        option,
                    })
                    .collect(),
                selected,
            }
        };
        let choices = vec![
            choice(
                StrKey::DrawChoiceFlanks,
                vec![
                    (
                        StrKey::DrawChoiceFlanksInvolute,
                        ToolOption::Flanks(GearFlanks::Involute),
                    ),
                    (
                        StrKey::DrawChoiceFlanksStraight,
                        ToolOption::Flanks(GearFlanks::Straight),
                    ),
                ],
                ToolOption::Flanks(gear.flanks),
            ),
            choice(
                StrKey::DrawChoiceRoot,
                vec![
                    (
                        StrKey::DrawChoiceRootRounded,
                        ToolOption::Root(GearRoot::Rounded),
                    ),
                    (StrKey::DrawChoiceRootFlat, ToolOption::Root(GearRoot::Flat)),
                    (
                        StrKey::DrawChoiceRootSharp,
                        ToolOption::Root(GearRoot::Sharp),
                    ),
                ],
                ToolOption::Root(gear.root),
            ),
            choice(
                StrKey::DrawChoiceCrest,
                vec![
                    (
                        StrKey::DrawChoiceCrestFlat,
                        ToolOption::Crest(GearCrest::Flat),
                    ),
                    (
                        StrKey::DrawChoiceCrestRounded,
                        ToolOption::Crest(GearCrest::Rounded),
                    ),
                ],
                ToolOption::Crest(gear.crest),
            ),
        ];
        // The module, derived: shown while the sizes give one.
        let module = gear.module_mm();
        let note = (module.is_finite() && module > 0.0).then(|| {
            strings.fmt(
                StrKey::DrawNoteModule,
                &[("module", &units::summary_text(module, units))],
            )
        });
        ToolBar {
            tool,
            prompt: strings.get(StrKey::DrawPromptGearSizes).to_owned(),
            fields,
            choices,
            note,
            can_finish: problems.is_empty(),
        }
    }

    /// Why the gear cannot be, in project units where the reason names a
    /// limit; upstream's words (`validateGearProfileParams`).
    fn gear_problem_text(&self, problem: &GearProblem) -> String {
        let units = self.project().display_units;
        let strings = self.strings();
        match *problem {
            GearProblem::RootFilletTooLarge { max_mm } => strings.fmt(
                StrKey::DrawProblemRootFilletTooLarge,
                &[("max", &units::limit_text(max_mm, units))],
            ),
            GearProblem::CrestRadiusTooLarge { max_mm } => strings.fmt(
                StrKey::DrawProblemCrestRadiusTooLarge,
                &[("max", &units::limit_text(max_mm, units))],
            ),
            GearProblem::BoreTooLarge { root_diameter_mm } => strings.fmt(
                StrKey::DrawProblemBoreTooLarge,
                &[("root", &units::limit_text(root_diameter_mm, units))],
            ),
            other => other.to_string(),
        }
    }

    /// To `state`, with nothing typed for its step.
    fn restart_gear(&mut self, state: GearDraft) {
        self.tool.gear = state;
        self.tool.pins.clear();
        self.tool.problems.clear();
    }

    /// The pointer's point for the step in hand, snapped, the centre its
    /// reference.
    fn gear_pointer(&self) -> Option<Snapped> {
        let pointer = self.tool.pointer?;
        let reference = match self.tool.gear {
            GearDraft::Centre(centre) => Some(centre),
            _ => None,
        };
        Some(self.snap_at(&pointer, reference, &[]))
    }

    /// The centre as the first step would take it: the pointer's, with a
    /// typed X or Y from the origin in its place.
    fn gear_centre(&self) -> Option<Point> {
        let project = self.project();
        let origin = project.origin.xy.point_mm(&project.stock);
        let pin = |field: SizeField| self.tool.pins.get(&field).copied();
        let pointer = self.gear_pointer().map(|snapped| snapped.at_mm);
        let x = pin(SizeField::X)
            .map(|x| origin.x + x)
            .or(pointer.map(|p| p.x))?;
        let y = pin(SizeField::Y)
            .map(|y| origin.y + y)
            .or(pointer.map(|p| p.y))?;
        Some(Point::new(x, y))
    }

    /// The outside radius as the second step would take it: half the
    /// typed diameter, else the pointer's distance from the centre.
    fn gear_radius_mm(&self, centre: Point) -> Option<f64> {
        self.tool
            .pins
            .get(&SizeField::Diameter)
            .map(|diameter| diameter / 2.0)
            .or_else(|| {
                self.gear_pointer()
                    .map(|snapped| centre.distance_to(snapped.at_mm))
            })
    }

    /// What the fields of the first two steps read.
    fn gear_step_values(&self) -> Vec<(SizeField, f64)> {
        let project = self.project();
        let origin = project.origin.xy.point_mm(&project.stock);
        match self.tool.gear {
            GearDraft::Idle => self
                .gear_centre()
                .map(|centre| {
                    vec![
                        (SizeField::X, centre.x - origin.x),
                        (SizeField::Y, centre.y - origin.y),
                    ]
                })
                .unwrap_or_default(),
            GearDraft::Centre(centre) => self
                .gear_radius_mm(centre)
                .map(|radius| vec![(SizeField::Diameter, radius * 2.0)])
                .unwrap_or_default(),
            GearDraft::Sizes { .. } => Vec::new(),
        }
    }

    /// Takes the step's point: the centre, then the outside radius
    /// (`setPendingGearRadiusAt`), which gives the gear its defaults. A
    /// click in the Viewport once the sizes are up does nothing.
    fn gear_point(&mut self) {
        match self.tool.gear {
            GearDraft::Idle => {
                if let Some(centre) = self.gear_centre() {
                    self.restart_gear(GearDraft::Centre(centre));
                }
            }
            GearDraft::Centre(centre) => {
                if let Some(radius) = self.gear_radius_mm(centre)
                    && radius * 2.0 >= MIN_SIZE_MM
                {
                    self.restart_gear(GearDraft::Sizes {
                        centre,
                        gear: Gear::with_defaults(radius),
                    });
                }
            }
            GearDraft::Sizes { .. } => {}
        }
    }

    /// Reads a size typed in the sizes step (`setPendingGearParams`): the
    /// gear holds it as typed, right or wrong. Text that is no number
    /// leaves the size and says so at its field; empty text leaves it
    /// too, but for the bore, which it takes away.
    fn type_gear_size(&mut self, field: SizeField, text: &str) {
        let GearDraft::Sizes { centre, mut gear } = self.tool.gear else {
            return;
        };
        self.tool.problems.remove(&field);
        let text = text.trim();
        if text.is_empty() {
            if field == SizeField::Bore {
                gear.bore_diameter_mm = None;
                self.tool.gear = GearDraft::Sizes { centre, gear };
            }
            return;
        }
        let value = match self.read_size(field, text) {
            Ok(value) => value,
            Err(problem) => {
                self.tool.problems.insert(field, problem);
                return;
            }
        };
        match field {
            SizeField::Teeth => {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "read from a u32 just above"
                )]
                let teeth = value as u32;
                gear.teeth = teeth;
            }
            SizeField::Diameter => gear.outside_radius_mm = value / 2.0,
            SizeField::PressureAngle => gear.pressure_angle_deg = value,
            SizeField::WholeDepth => gear.whole_depth_mm = value,
            SizeField::RootFillet => gear.root_fillet_mm = value,
            SizeField::CrestRadius => gear.crest_radius_mm = value,
            // A bore of 0 is no bore.
            SizeField::Bore => gear.bore_diameter_mm = (value.abs() > 0.0).then_some(value),
            _ => {}
        }
        self.tool.gear = GearDraft::Sizes { centre, gear };
    }

    /// Makes the gear a shape (`completePendingGear`), its bore a second
    /// outline, placed by its centre. Refused, with the first reason,
    /// while its sizes give no gear.
    fn make_gear(&mut self, centre: Point, gear: &Gear) -> Reply {
        let profiles = match gear.outline() {
            Ok(profiles) => profiles,
            Err(problem) => {
                return Reply {
                    ran: None,
                    refused: Some(self.gear_problem_text(&problem)),
                };
            }
        };
        let name = next_name(self.project(), self.strings().get(StrKey::DrawNameGear));
        let command = Command::AddShapes(vec![NewShape {
            name,
            shape: Some(Shape::from_gear(gear)),
            profiles,
            transform: Transform::translate(centre.x, centre.y),
            construction: self.tool.construction,
            folder: self.selection_folder(),
        }]);
        match self.run(command) {
            Ok(label) => {
                self.restart_gear(GearDraft::Idle);
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

    /// What the tool wants drawn: the gear as it would be made now, with
    /// its defaults before the radius is taken; the outside circle alone
    /// while the sizes give no gear.
    fn refresh_gear_draft(&mut self) {
        let units = self.project().display_units;
        let mut draft = Draft::default();
        let shown = match self.tool.gear {
            GearDraft::Idle => {
                draft.snap = self.gear_pointer();
                None
            }
            GearDraft::Centre(centre) => {
                draft.snap = self.gear_pointer();
                self.gear_radius_mm(centre)
                    .filter(|radius| radius * 2.0 >= MIN_SIZE_MM)
                    .map(|radius| (centre, Gear::with_defaults(radius), true))
            }
            GearDraft::Sizes { centre, gear } => Some((centre, gear, false)),
        };
        if let Some((centre, gear, labelled)) = shown {
            let outlines: Vec<Polyline> = gear.outline().unwrap_or_else(|_| {
                let diameter = gear.outside_radius_mm * 2.0;
                if diameter.is_finite() && diameter >= MIN_SIZE_MM {
                    vec![shapes::circle(diameter)]
                } else {
                    Vec::new()
                }
            });
            let place = Transform::translate(centre.x, centre.y);
            draft.outlines = outlines
                .iter()
                .filter_map(|outline| place.apply_polyline(outline).ok())
                .collect();
            if labelled {
                draft.labels.push(Label {
                    at_mm: Point::new(centre.x, centre.y - gear.outside_radius_mm),
                    text: self.strings().fmt(
                        StrKey::DrawLabelDiameter,
                        &[(
                            "diameter",
                            &units::summary_text(gear.outside_radius_mm * 2.0, units),
                        )],
                    ),
                });
            }
        }
        self.tool.draft = draft;
    }
}

/// A field's label in the Gear tool: its diameter is the outside one.
const fn gear_label(field: SizeField) -> StrKey {
    match field {
        SizeField::Diameter => StrKey::DrawFieldOutsideDiameter,
        other => field_label(other),
    }
}
