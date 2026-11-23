// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! **Arrange ▸ Array…** as a mode of the Viewport (functional spec,
//! capability 98): the tool bar takes the array's kind, its counts and
//! its spacing or angle, the copies are previewed as the values change,
//! and a circular array's centre is typed or clicked on the Viewport,
//! with snapping. **Done** makes the copies ([`crate::array::command`])
//! and leaves the whole array selected.

use dc_geometry::Point;

use super::{
    Adjust, Choice, ChoiceOption, Draft, Event, Key, Quantity, Reply, SizeField, SizeRow, Tool,
    ToolBar, ToolOption,
};
use crate::app::App;
use crate::array::{self, Array, ArrayError, Circular, Grid, GridSpacing};
use crate::command::{Command, selection_bounds};
use crate::error::CommandError;
use crate::strings::StrKey;
use crate::units;

/// Which array the bar is setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayKind {
    /// Rows and columns.
    Grid,
    /// Round a centre.
    Circular,
}

/// What the Array bar was last given: kept while the app runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrayPrefs {
    /// Grid or circular.
    pub kind: ArrayKind,
    /// A grid's columns.
    pub columns: u32,
    /// A grid's rows.
    pub rows: u32,
    /// A grid's spacing along X, mm; `None` until one is typed.
    pub x_mm: Option<f64>,
    /// A grid's spacing along Y, mm; `None` until one is typed.
    pub y_mm: Option<f64>,
    /// What a grid's spacings measure.
    pub spacing: GridSpacing,
    /// How many copies a circular array has, the source among them.
    pub count: u32,
    /// The angle a circular array is spread over, degrees.
    pub total_deg: f64,
    /// Whether a circular array's copies are turned.
    pub turn: bool,
}

impl Default for ArrayPrefs {
    /// Two columns in a row with a gap between; four turned copies
    /// round a whole turn.
    fn default() -> Self {
        Self {
            kind: ArrayKind::Grid,
            columns: 2,
            rows: 1,
            x_mm: None,
            y_mm: None,
            spacing: GridSpacing::Gap,
            count: 4,
            total_deg: 360.0,
            turn: true,
        }
    }
}

/// A grid's spacing until one is typed: PureCutCNC's 20 mm, or 2 in.
const fn default_spacing_mm(units: dc_project::Units) -> f64 {
    match units {
        dc_project::Units::Mm => 20.0,
        dc_project::Units::Inch => 50.8,
    }
}

/// The field an array's refusal is said at.
const fn field_of(error: ArrayError, kind: ArrayKind) -> SizeField {
    match (error, kind) {
        (ArrayError::NoSpacing, _) => SizeField::SpacingX,
        (ArrayError::NoAngle, _) => SizeField::Angle,
        (ArrayError::CentreOnSelection | ArrayError::NoCentre, _) => SizeField::X,
        (_, ArrayKind::Grid) => SizeField::Columns,
        (_, ArrayKind::Circular) => SizeField::Copies,
    }
}

impl App {
    /// The array the bar reads; a circular one has none until its centre
    /// is given.
    fn array_asked(&self) -> Result<Array, ArrayError> {
        let prefs = self.prefs().adjust.array;
        let spacing = default_spacing_mm(self.project().display_units);
        match prefs.kind {
            ArrayKind::Grid => Ok(Array::Grid(Grid {
                columns: prefs.columns,
                rows: prefs.rows,
                x_mm: prefs.x_mm.unwrap_or(spacing),
                y_mm: prefs.y_mm.unwrap_or(spacing),
                spacing: prefs.spacing,
            })),
            ArrayKind::Circular => Ok(Array::Circular(Circular {
                count: prefs.count,
                centre: self.tool.array_centre.ok_or(ArrayError::NoCentre)?,
                total_deg: prefs.total_deg,
                turn: prefs.turn,
            })),
        }
    }

    /// The command **Done** would run now.
    fn array_command(&self) -> Result<Command, CommandError> {
        let ids = self.selection().instances();
        if ids.is_empty() {
            return Err(CommandError::NothingSelected);
        }
        let asked = self.array_asked()?;
        array::command(self.project(), self.strings(), ids, &asked)
    }

    /// Hands `event` to the Array mode.
    pub(super) fn array_event(&mut self, event: Event) -> Reply {
        let reply = match event {
            Event::Key(Key::Escape) => {
                self.arm(Tool::Select);
                return Reply::default();
            }
            Event::Key(Key::Enter) | Event::Done => {
                let ran = self.array_command().and_then(|command| self.run(command));
                match ran {
                    Ok(label) => {
                        // The array is selected as a click on it would
                        // select it: the group, its source too.
                        let all =
                            crate::tree::with_groups(self.project(), self.selection().instances());
                        self.select(crate::Selection::Instances(all));
                        self.arm(Tool::Select);
                        return Reply {
                            ran: Some(label),
                            refused: None,
                        };
                    }
                    Err(error) => Reply {
                        ran: None,
                        refused: Some(error.to_string()),
                    },
                }
            }
            Event::Field { field, text } => {
                self.type_array(field, &text);
                Reply::default()
            }
            Event::Option(ToolOption::ArrayKind(kind)) => {
                self.prefs_mut().adjust.array.kind = kind;
                self.tool.problems.clear();
                Reply::default()
            }
            Event::Option(ToolOption::ArraySpacing(spacing)) => {
                self.prefs_mut().adjust.array.spacing = spacing;
                Reply::default()
            }
            Event::Option(ToolOption::ArrayTurn(turn)) => {
                self.prefs_mut().adjust.array.turn = turn;
                Reply::default()
            }
            // A click on the Viewport is a circular array's centre.
            Event::Press(pointer) if self.prefs().adjust.array.kind == ArrayKind::Circular => {
                let snapped = self.snap_at(&pointer, None, &[]);
                self.tool.array_centre = Some(snapped.at_mm);
                self.tool.problems.clear();
                Reply::default()
            }
            Event::Move(pointer) if self.prefs().adjust.array.kind == ArrayKind::Circular => {
                self.tool.pointer = Some(pointer);
                Reply::default()
            }
            Event::Leave => {
                self.tool.pointer = None;
                Reply::default()
            }
            _ => Reply::default(),
        };
        self.refresh_array_draft();
        reply
    }

    /// A value typed in the bar: kept, or refused at its field.
    fn type_array(&mut self, field: SizeField, text: &str) {
        self.tool.problems.remove(&field);
        if text.trim().is_empty() {
            return;
        }
        let value = match self.read_size(field, text) {
            Ok(value) => value,
            Err(problem) => {
                self.tool.problems.insert(field, problem);
                return;
            }
        };
        let project = self.project();
        let origin = project.origin.xy.point_mm(&project.stock);
        // A centre typed one coordinate at a time starts level with the
        // middle of what is copied.
        let middle = selection_bounds(project, self.selection().instances())
            .map(|b| {
                Point::new(
                    f64::midpoint(b.min.x, b.max.x),
                    f64::midpoint(b.min.y, b.max.y),
                )
            })
            .unwrap_or(origin);
        let centre = self.tool.array_centre.unwrap_or(middle);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a count read as a whole number that fits"
        )]
        let count = value as u32;
        if field.quantity() == Quantity::Count && count == 0 {
            let problem = self
                .strings()
                .get(StrKey::DrawProblemNotPositive)
                .to_owned();
            self.tool.problems.insert(field, problem);
            return;
        }
        let prefs = &mut self.prefs_mut().adjust.array;
        match field {
            SizeField::Columns => prefs.columns = count,
            SizeField::Rows => prefs.rows = count,
            SizeField::Copies => prefs.count = count,
            SizeField::SpacingX => prefs.x_mm = Some(value),
            SizeField::SpacingY => prefs.y_mm = Some(value),
            SizeField::Angle => prefs.total_deg = value,
            SizeField::X => self.tool.array_centre = Some(Point::new(origin.x + value, centre.y)),
            SizeField::Y => self.tool.array_centre = Some(Point::new(centre.x, origin.y + value)),
            _ => {}
        }
    }

    /// The preview: each copy's outlines where it would go, and, while a
    /// circular array's centre is being chosen, where a click would put
    /// it.
    pub(super) fn refresh_array_draft(&mut self) {
        let mut draft = Draft::default();
        let project = self.project();
        let ids = self.selection().instances();
        if let Ok(asked) = self.array_asked()
            && let Ok(bounds) = selection_bounds(project, ids)
            && let Ok(placements) = array::placements(&asked, bounds)
        {
            for placed in placements {
                for id in ids {
                    let Some(instance) = project.instances.iter().find(|i| i.id == *id) else {
                        continue;
                    };
                    let Some(definition) = project.definitions.get(&instance.definition) else {
                        continue;
                    };
                    let transform = instance.transform.then(placed);
                    draft.outlines.extend(
                        definition
                            .profiles
                            .iter()
                            .filter_map(|profile| transform.apply_polyline(profile).ok()),
                    );
                }
            }
        }
        if self.prefs().adjust.array.kind == ArrayKind::Circular
            && let Some(pointer) = self.tool.pointer
        {
            draft.snap = Some(self.snap_at(&pointer, None, &[]));
        }
        self.tool.draft = draft;
    }

    /// The Array mode's tool bar.
    pub(super) fn array_bar(&self) -> ToolBar {
        let strings = self.strings();
        let project = self.project();
        let units = project.display_units;
        let prefs = self.prefs().adjust.array;
        let ready = self.array_command();
        let blocked = match &ready {
            Ok(_) | Err(CommandError::NothingSelected) => None,
            Err(CommandError::Array(error)) => {
                Some((field_of(*error, prefs.kind), error.to_string()))
            }
            Err(error) => Some((
                match prefs.kind {
                    ArrayKind::Grid => SizeField::Columns,
                    ArrayKind::Circular => SizeField::Copies,
                },
                error.to_string(),
            )),
        };
        let prompt = match (&ready, prefs.kind, self.tool.array_centre) {
            (Err(CommandError::NothingSelected), _, _) => StrKey::AdjustPromptSelect,
            (_, ArrayKind::Grid, _) => StrKey::ArrayPromptGrid,
            (_, ArrayKind::Circular, None) => StrKey::ArrayPromptCentre,
            (_, ArrayKind::Circular, Some(_)) => StrKey::ArrayPromptCircular,
        };
        let row = |field: SizeField, text: String| SizeRow {
            field,
            label: strings.get(field.label()).to_owned(),
            help: strings.get(field.help()).to_owned(),
            text,
            quantity: field.quantity(),
            pinned: false,
            // What the field itself refused, else what stands in the way
            // of Done and is this field's to mend; a missing centre is
            // the prompt's to say.
            problem: self.tool.problems.get(&field).cloned().or_else(|| {
                blocked
                    .as_ref()
                    .filter(|(at, _)| *at == field)
                    .filter(|_| !matches!(ready, Err(CommandError::Array(ArrayError::NoCentre))))
                    .map(|(_, reason)| reason.clone())
            }),
            option: true,
        };
        let length = |mm: f64| units::field_text(mm, units);
        let spacing = default_spacing_mm(units);
        let origin = project.origin.xy.point_mm(&project.stock);
        let choice = |label: StrKey,
                      help: StrKey,
                      options: Vec<(StrKey, ToolOption)>,
                      chosen: ToolOption| Choice {
            label: strings.get(label).to_owned(),
            help: strings.get(help).to_owned(),
            selected: options
                .iter()
                .position(|(_, option)| *option == chosen)
                .unwrap_or(0),
            options: options
                .into_iter()
                .map(|(label, option)| ChoiceOption {
                    label: strings.get(label).to_owned(),
                    option,
                })
                .collect(),
        };
        let kind = choice(
            StrKey::ArrayKind,
            StrKey::ArrayKindHelp,
            vec![
                (
                    StrKey::ArrayKindGrid,
                    ToolOption::ArrayKind(ArrayKind::Grid),
                ),
                (
                    StrKey::ArrayKindCircular,
                    ToolOption::ArrayKind(ArrayKind::Circular),
                ),
            ],
            ToolOption::ArrayKind(prefs.kind),
        );
        let (fields, second) = match prefs.kind {
            ArrayKind::Grid => (
                vec![
                    row(SizeField::Columns, prefs.columns.to_string()),
                    row(SizeField::Rows, prefs.rows.to_string()),
                    row(SizeField::SpacingX, length(prefs.x_mm.unwrap_or(spacing))),
                    row(SizeField::SpacingY, length(prefs.y_mm.unwrap_or(spacing))),
                ],
                choice(
                    StrKey::ArraySpacing,
                    StrKey::ArraySpacingHelp,
                    vec![
                        (
                            StrKey::ArraySpacingGap,
                            ToolOption::ArraySpacing(GridSpacing::Gap),
                        ),
                        (
                            StrKey::ArraySpacingPitch,
                            ToolOption::ArraySpacing(GridSpacing::Pitch),
                        ),
                    ],
                    ToolOption::ArraySpacing(prefs.spacing),
                ),
            ),
            ArrayKind::Circular => {
                let centre = self.tool.array_centre;
                let coordinate = |of: fn(Point) -> f64| {
                    centre.map_or_else(String::new, |at| length(of(at) - of(origin)))
                };
                (
                    vec![
                        row(SizeField::Copies, prefs.count.to_string()),
                        row(SizeField::X, coordinate(|p| p.x)),
                        row(SizeField::Y, coordinate(|p| p.y)),
                        row(
                            SizeField::Angle,
                            super::size_text(SizeField::Angle, prefs.total_deg, units),
                        ),
                    ],
                    choice(
                        StrKey::ArrayTurn,
                        StrKey::ArrayTurnHelp,
                        vec![
                            (StrKey::ArrayTurnOn, ToolOption::ArrayTurn(true)),
                            (StrKey::ArrayTurnOff, ToolOption::ArrayTurn(false)),
                        ],
                        ToolOption::ArrayTurn(prefs.turn),
                    ),
                )
            }
        };
        ToolBar {
            tool: Tool::Adjust(Adjust::Array),
            prompt: strings.get(prompt).to_owned(),
            can_finish: ready.is_ok() && self.tool.problems.is_empty(),
            fields,
            choices: vec![kind, second],
            actions: Vec::new(),
            note: None,
        }
    }
}
