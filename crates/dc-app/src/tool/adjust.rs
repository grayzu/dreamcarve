// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Shape menu's commands that take a value before they act
//! (`docs/specs/phase-5/design.md`, "The window (slice 17)"; functional
//! capabilities 93, 94 and 97): **Offset…**, **Fillet Corners…** and
//! **Join**, each a mode of the Viewport with the tool bar for its
//! popover.
//!
//! The functional spec has a popover for each. A mode with the tool bar
//! is the same thing with what is already there: the bar holds the
//! value's field, the choices and **Done**, the draft is the live
//! preview, and the whole of it is tested with no window. The selection
//! it acts on is the one it was armed with. **Done** or `Enter` runs the
//! one command and returns to Select with the result selected; `Escape`
//! returns with nothing done.

use dc_project::Units;

use super::{
    Adjust, Choice, ChoiceOption, Draft, Event, Key, Reply, SizeField, SizeRow, ToolBar, ToolOption,
};
use crate::edit::{self, Offset, OffsetSide};
use crate::strings::StrKey;
use crate::{App, Command, CommandError, Tool, units};

/// What the Shape menu's value-taking commands were last given: kept
/// while the app runs, so the next offset starts from the last.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdjustPrefs {
    /// The offset's distance, mm; `None` until one is typed.
    pub offset_mm: Option<f64>,
    /// Which side the offset is made on.
    pub offset_side: OffsetSide,
    /// Whether the shape offset stays.
    pub keep_original: bool,
    /// Fillet Corners' radius, mm; `None` until one is typed.
    pub fillet_mm: Option<f64>,
    /// How near two ends must be to be joined, mm.
    pub join_tolerance_mm: f64,
    /// Whether Align aligns to the stock whatever is selected
    /// (capability 98a).
    pub align_to_stock: bool,
    /// What Array… was last given (capability 98).
    pub array: super::ArrayPrefs,
}

impl Default for AdjustPrefs {
    /// Outward, the original kept, ends joined within 0.1 mm.
    fn default() -> Self {
        Self {
            offset_mm: None,
            offset_side: OffsetSide::Outward,
            keep_original: true,
            fillet_mm: None,
            join_tolerance_mm: edit::JOIN_TOLERANCE_MM,
            align_to_stock: false,
            array: super::ArrayPrefs::default(),
        }
    }
}

impl AdjustPrefs {
    /// The value `adjust` starts from in a project of `units`: the one
    /// last typed, else the corner size's default for a distance or a
    /// radius (5 mm or 0.2 in).
    fn value_mm(&self, adjust: Adjust, units: Units) -> f64 {
        let default = crate::prefs::default_corner_mm(units);
        match adjust {
            Adjust::Offset => self.offset_mm.unwrap_or(default),
            Adjust::FilletCorners => self.fillet_mm.unwrap_or(default),
            Adjust::Join => self.join_tolerance_mm,
            // An array's values are its own (`tool::arrays`), and a
            // picture's scale its own (`tool::scale`).
            Adjust::Array | Adjust::PictureScale => 0.0,
        }
    }
}

/// The field `adjust` takes its value in.
const fn field_of(adjust: Adjust) -> SizeField {
    match adjust {
        Adjust::Offset => SizeField::Distance,
        Adjust::FilletCorners => SizeField::Radius,
        Adjust::Join => SizeField::Tolerance,
        Adjust::Array => SizeField::Columns,
        Adjust::PictureScale => SizeField::Distance,
    }
}

impl App {
    /// The offset the bar reads.
    fn offset_asked(&self) -> Offset {
        let prefs = self.prefs().adjust;
        Offset {
            distance_mm: prefs.value_mm(Adjust::Offset, self.project().display_units),
            side: prefs.offset_side,
            keep_original: prefs.keep_original,
        }
    }

    /// The command **Done** would run now, with what there is to say
    /// beside it (the shapes an offset vanished for, the paths a join
    /// left alone).
    fn adjust_command(&self, adjust: Adjust) -> Result<(Command, Option<String>), CommandError> {
        let project = self.project();
        let strings = self.strings();
        let ids = self.selection().instances();
        let units = project.display_units;
        match adjust {
            Adjust::Offset => {
                let made = edit::offset(project, strings, ids, &self.offset_asked())?;
                let note = (!made.vanished.is_empty()).then(|| {
                    strings.fmt(
                        StrKey::EditRefusedOffsetNothing,
                        &[("names", &made.vanished.join(", "))],
                    )
                });
                Ok((made.command, note))
            }
            Adjust::FilletCorners => {
                let [id] = ids else {
                    return Err(CommandError::NotAQuantity {
                        text: String::new(),
                        reason: strings.get(StrKey::AdjustRefusedOneShape).to_owned(),
                    });
                };
                let radius = self.prefs().adjust.value_mm(adjust, units);
                let text = units::field_text(radius, units);
                Ok((edit::fillet_corners(project, strings, id, &text)?, None))
            }
            Adjust::Array | Adjust::PictureScale => Err(CommandError::NothingToDo),
            Adjust::Join => {
                let joined =
                    edit::join(project, strings, ids, self.prefs().adjust.join_tolerance_mm)?;
                let note = (joined.alone > 0).then(|| {
                    strings.fmt(
                        StrKey::AdjustJoinAlone,
                        &[("count", &joined.alone.to_string())],
                    )
                });
                Ok((joined.command, note))
            }
        }
    }

    /// Hands `event` to the armed Shape command `adjust`.
    pub(super) fn adjust_event(&mut self, adjust: Adjust, event: Event) -> Reply {
        match adjust {
            Adjust::Array => return self.array_event(event),
            Adjust::PictureScale => return self.scale_event(event),
            _ => {}
        }
        let reply = match event {
            Event::Key(Key::Escape) => {
                self.arm(Tool::Select);
                return Reply::default();
            }
            Event::Key(Key::Enter) | Event::Done => match self.adjust_command(adjust) {
                Ok((command, note)) => match self.run(command) {
                    Ok(label) => {
                        // What was made is selected; the mode is over.
                        self.arm(Tool::Select);
                        return Reply {
                            ran: Some(label),
                            refused: note,
                        };
                    }
                    Err(error) => Reply {
                        ran: None,
                        refused: Some(error.to_string()),
                    },
                },
                Err(error) => Reply {
                    ran: None,
                    refused: Some(error.to_string()),
                },
            },
            Event::Field { field, text } => {
                self.type_adjust(adjust, field, &text);
                Reply::default()
            }
            Event::Option(ToolOption::OffsetSide(side)) => {
                self.prefs_mut().adjust.offset_side = side;
                Reply::default()
            }
            Event::Option(ToolOption::KeepOriginal(keep)) => {
                self.prefs_mut().adjust.keep_original = keep;
                Reply::default()
            }
            _ => Reply::default(),
        };
        self.refresh_adjust_draft(adjust);
        reply
    }

    /// A value typed in the bar: kept, or refused at its field.
    fn type_adjust(&mut self, adjust: Adjust, field: SizeField, text: &str) {
        self.tool.problems.remove(&field);
        if field != field_of(adjust) || text.trim().is_empty() {
            return;
        }
        match self.read_size(field, text) {
            Ok(value) if value > 0.0 => {
                let prefs = &mut self.prefs_mut().adjust;
                match adjust {
                    Adjust::Offset => prefs.offset_mm = Some(value),
                    Adjust::FilletCorners => prefs.fillet_mm = Some(value),
                    Adjust::Join => prefs.join_tolerance_mm = value,
                    Adjust::Array | Adjust::PictureScale => {}
                }
            }
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

    /// The preview: an offset as it would be made.
    pub(super) fn refresh_adjust_draft(&mut self, adjust: Adjust) {
        match adjust {
            Adjust::Array => return self.refresh_array_draft(),
            Adjust::PictureScale => return self.refresh_scale_draft(),
            _ => {}
        }
        let mut draft = Draft::default();
        if adjust == Adjust::Offset {
            draft.outlines = edit::offset_outlines(
                self.project(),
                self.selection().instances(),
                &self.offset_asked(),
            );
        }
        self.tool.draft = draft;
    }

    /// Whether something is selected and every outline of it is open.
    fn selection_is_open(&self) -> bool {
        let project = self.project();
        let selected = self.selection().instances();
        !selected.is_empty()
            && selected.iter().all(|id| {
                project
                    .instances
                    .iter()
                    .find(|instance| instance.id == *id)
                    .and_then(|instance| project.definitions.get(&instance.definition))
                    .is_some_and(|definition| {
                        definition
                            .profiles
                            .iter()
                            .all(|profile| !profile.is_closed())
                    })
            })
    }

    /// The tool bar for the armed Shape command `adjust`.
    pub(super) fn adjust_bar(&self, adjust: Adjust) -> ToolBar {
        match adjust {
            Adjust::Array => return self.array_bar(),
            Adjust::PictureScale => return self.scale_bar(),
            _ => {}
        }
        let strings = self.strings();
        let units = self.project().display_units;
        let prefs = self.prefs().adjust;
        let field = field_of(adjust);
        let ready = self.adjust_command(adjust);
        let prompt = match (adjust, &ready) {
            (_, Err(CommandError::NothingSelected)) => StrKey::AdjustPromptSelect,
            (Adjust::Offset, _) => StrKey::AdjustPromptOffset,
            (Adjust::FilletCorners, _) => StrKey::AdjustPromptFillet,
            (Adjust::Join | Adjust::Array | Adjust::PictureScale, _) => StrKey::AdjustPromptJoin,
        };
        let choice = |label: StrKey,
                      help: StrKey,
                      options: Vec<(StrKey, ToolOption)>,
                      chosen: ToolOption| Choice {
            label: strings.get(label).to_owned(),
            help: strings.get(help).to_owned(),
            list: false,
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
        // An open path has no inside: its sides are the left and the
        // right of its direction.
        let (outward, inward) = if self.selection_is_open() {
            (StrKey::AdjustSideLeft, StrKey::AdjustSideRight)
        } else {
            (StrKey::AdjustSideOutward, StrKey::AdjustSideInward)
        };
        let choices = if adjust == Adjust::Offset {
            vec![
                choice(
                    StrKey::AdjustSide,
                    StrKey::AdjustSideHelp,
                    vec![
                        (outward, ToolOption::OffsetSide(OffsetSide::Outward)),
                        (inward, ToolOption::OffsetSide(OffsetSide::Inward)),
                        (
                            StrKey::AdjustSideBoth,
                            ToolOption::OffsetSide(OffsetSide::Both),
                        ),
                    ],
                    ToolOption::OffsetSide(prefs.offset_side),
                ),
                choice(
                    StrKey::AdjustOriginal,
                    StrKey::AdjustOriginalHelp,
                    vec![
                        (StrKey::AdjustOriginalKeep, ToolOption::KeepOriginal(true)),
                        (
                            StrKey::AdjustOriginalReplace,
                            ToolOption::KeepOriginal(false),
                        ),
                    ],
                    ToolOption::KeepOriginal(prefs.keep_original),
                ),
            ]
        } else {
            Vec::new()
        };
        // What stands in the way of Done, said where the value is typed
        // unless the field has its own complaint.
        let blocked = match &ready {
            Err(CommandError::NothingSelected) | Ok(_) => None,
            Err(error) => Some(error.to_string()),
        };
        ToolBar {
            readings: Vec::new(),
            tool: Tool::Adjust(adjust),
            prompt: strings.get(prompt).to_owned(),
            fields: vec![SizeRow {
                field,
                label: strings.get(field.label()).to_owned(),
                help: strings.get(field.help()).to_owned(),
                text: units::field_text(prefs.value_mm(adjust, units), units),
                quantity: field.quantity(),
                pinned: false,
                problem: self.tool.problems.get(&field).cloned().or(blocked),
                option: true,
            }],
            choices,
            actions: Vec::new(),
            note: None,
            can_finish: ready.is_ok(),
            text: None,
        }
    }
}
