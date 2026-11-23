// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! **Set scale…** as a mode of the Viewport (functional spec, capability
//! 101): a picture scaled by two points. Two clicks on the one picture
//! selected give two points of it; the tool bar then takes the real
//! distance between them, and **Done** resizes the picture about the
//! first point so that they are that far apart
//! ([`crate::pictures::scale_by_two_points`]).
//!
//! The points are taken where the pointer is: a picture does not snap,
//! and what is clicked is a mark in the picture, which no shape, stock
//! edge or grid line knows about. A third click moves the second point.
//! `Escape` takes the last point back, and with none leaves the mode.

use dc_geometry::{Point, Polyline, Vertex};
use dc_project::BitmapId;

use super::{Adjust, Draft, Event, Key, Label, NodeMark, Reply, SizeField, SizeRow, Tool, ToolBar};
use crate::app::App;
use crate::command::Command;
use crate::error::CommandError;
use crate::pictures::{self, MIN_SCALE_SPAN_MM};
use crate::strings::StrKey;
use crate::units;

/// How far *Set scale…* has got.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct ScaleDraft {
    /// The first point clicked, mm: the one the picture is resized about.
    first: Option<Point>,
    /// The second, mm.
    second: Option<Point>,
    /// The real distance between them, mm, once it is typed.
    distance_mm: Option<f64>,
}

impl App {
    /// The one picture selected, which is the one scaled.
    fn scale_picture(&self) -> Option<&BitmapId> {
        match self.selected_pictures() {
            [id] => Some(id),
            _ => None,
        }
    }

    /// The command **Done** would run now.
    fn scale_command(&self) -> Result<Command, CommandError> {
        let id = self.scale_picture().ok_or(CommandError::NothingSelected)?;
        let scale = self.tool.scale;
        let (Some(first), Some(second)) = (scale.first, scale.second) else {
            return Err(CommandError::NothingToDo);
        };
        let distance_mm = scale
            .distance_mm
            .unwrap_or_else(|| first.distance_to(second));
        pictures::scale_by_two_points(self.project(), id, first, second, distance_mm)
    }

    /// Hands `event` to *Set scale…*.
    pub(super) fn scale_event(&mut self, event: Event) -> Reply {
        let reply = match event {
            Event::Key(Key::Escape) => {
                let scale = &mut self.tool.scale;
                if scale.second.take().is_some() {
                    scale.distance_mm = None;
                    self.tool.problems.clear();
                } else if scale.first.take().is_none() {
                    self.arm(Tool::Select);
                    return Reply::default();
                }
                Reply::default()
            }
            Event::Key(Key::Enter) | Event::Done => {
                match self.scale_command().and_then(|command| self.run(command)) {
                    Ok(label) => {
                        // The picture stays selected; the mode is over.
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
            Event::Press(pointer) if self.scale_picture().is_some() => {
                let at = pointer.at_mm;
                let scale = &mut self.tool.scale;
                match scale.first {
                    None => scale.first = Some(at),
                    // Two points that are one have no distance between.
                    Some(first) if first.distance_to(at) >= MIN_SCALE_SPAN_MM => {
                        scale.second = Some(at);
                    }
                    Some(_) => {}
                }
                Reply::default()
            }
            Event::Move(pointer) => {
                self.tool.pointer = Some(pointer);
                Reply::default()
            }
            Event::Leave => {
                self.tool.pointer = None;
                Reply::default()
            }
            Event::Field {
                field: SizeField::Distance,
                text,
            } => {
                self.type_scale(&text);
                Reply::default()
            }
            _ => Reply::default(),
        };
        self.refresh_scale_draft();
        reply
    }

    /// The real distance typed in the bar: kept, or refused at its field.
    /// Empty text is the distance the points have.
    fn type_scale(&mut self, text: &str) {
        let field = SizeField::Distance;
        self.tool.problems.remove(&field);
        if text.trim().is_empty() {
            self.tool.scale.distance_mm = None;
            return;
        }
        match self.read_size(field, text) {
            Ok(value) if value > 0.0 => self.tool.scale.distance_mm = Some(value),
            Ok(_) => {
                let problem = self
                    .strings()
                    .get(StrKey::DrawProblemNotPositive)
                    .to_owned();
                self.tool.problems.insert(field, problem);
            }
            Err(problem) => {
                self.tool.problems.insert(field, problem);
            }
        }
    }

    /// The preview: the points clicked, each marked, and the line from
    /// the first to the second, or to the pointer until there is one,
    /// with its length as the picture has it now.
    pub(super) fn refresh_scale_draft(&mut self) {
        let mut draft = Draft::default();
        let scale = self.tool.scale;
        draft.nodes = [scale.first, scale.second]
            .into_iter()
            .flatten()
            .map(|at_mm| NodeMark {
                at_mm,
                selected: false,
                smooth: true,
            })
            .collect();
        let to = scale
            .second
            .or_else(|| self.tool.pointer.map(|pointer| pointer.at_mm));
        if let (Some(first), Some(to)) = (scale.first, to)
            && first.distance_to(to) >= MIN_SCALE_SPAN_MM
        {
            draft.outlines.push(Polyline::new(
                vec![Vertex::line(first.x, first.y), Vertex::line(to.x, to.y)],
                false,
            ));
            draft.labels.push(Label {
                at_mm: first + (to - first) * 0.5,
                text: self.strings().fmt(
                    StrKey::DrawLabelLength,
                    &[(
                        "length",
                        &units::summary_text(first.distance_to(to), self.project().display_units),
                    )],
                ),
            });
        }
        self.tool.draft = draft;
    }

    /// *Set scale…*'s tool bar.
    pub(super) fn scale_bar(&self) -> ToolBar {
        let strings = self.strings();
        let units = self.project().display_units;
        let scale = self.tool.scale;
        let picture = self
            .scale_picture()
            .and_then(|id| self.project().bitmaps.iter().find(|b| b.id == *id));
        let prompt = match (picture, scale.first, scale.second) {
            (None, ..) => StrKey::ScalePromptSelect,
            (_, None, _) => StrKey::ScalePromptFirst,
            (_, _, None) => StrKey::ScalePromptSecond,
            _ => StrKey::ScalePromptDistance,
        };
        let ready = self.scale_command();
        // What stands in the way of Done, said where the distance is
        // typed unless the field has its own complaint.
        let blocked = match &ready {
            Ok(_) | Err(CommandError::NothingToDo | CommandError::NothingSelected) => None,
            Err(error) => Some(error.to_string()),
        };
        let field = SizeField::Distance;
        let fields = match (picture, scale.first, scale.second) {
            (Some(_), Some(first), Some(second)) => vec![SizeRow {
                field,
                label: strings.get(field.label()).to_owned(),
                help: strings.get(StrKey::ScaleDistanceHelp).to_owned(),
                text: units::field_text(
                    scale
                        .distance_mm
                        .unwrap_or_else(|| first.distance_to(second)),
                    units,
                ),
                quantity: field.quantity(),
                pinned: false,
                problem: self.tool.problems.get(&field).cloned().or(blocked),
                option: true,
            }],
            _ => Vec::new(),
        };
        // A locked picture is said to be before the clicks are spent.
        let note = picture.filter(|bitmap| bitmap.locked).map(|bitmap| {
            CommandError::PictureLocked {
                names: vec![bitmap.name.clone()],
            }
            .to_string()
        });
        ToolBar {
            readings: Vec::new(),
            tool: Tool::Adjust(Adjust::PictureScale),
            prompt: strings.get(prompt).to_owned(),
            fields,
            choices: Vec::new(),
            actions: Vec::new(),
            note,
            can_finish: ready.is_ok() && !self.tool.problems.contains_key(&field),
            text: None,
        }
    }
}
