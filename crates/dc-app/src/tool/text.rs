// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Text tool (phase 5 functional spec, capability 91): a click sets
//! where the first line's baseline starts; the tool bar takes the words,
//! previewed as they are typed, with the font, the height, the spacings
//! and the alignment, which are kept for the texts that follow; **Done**
//! makes one shape, *Text 1*, that keeps what it was made from.
//!
//! PureCutCNC's text is one line in a typeface.json font; this tool is
//! DreamCarve's own, on `dc-text`.

use dc_geometry::{Point, Transform};
use dc_project::{FontRef, Units};

use super::{Draft, Drawing, Event, Key, Reply, SizeField, Tool, ToolBar, ToolOption};
use crate::app::App;
use crate::command::{Command, NewShape, next_name};
use crate::strings::StrKey;
use crate::text::{self, TextSizes};

/// What the Text tool has in hand.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TextDraft {
    /// Where the first line's baseline starts, once clicked.
    at: Option<Point>,
    /// The words typed so far.
    words: String,
}

/// A text's height until one is typed: 10 mm, or 0.4 in.
const fn default_height_mm(units: Units) -> f64 {
    match units {
        Units::Mm => 10.0,
        Units::Inch => 10.16,
    }
}

impl App {
    /// The font the tool sets text in: the one last chosen when it is on
    /// hand, else the first there is.
    fn text_font(&self) -> Option<FontRef> {
        let chosen = &self.prefs().text.font;
        if self.fonts().find(chosen).is_some() {
            return Some(chosen.clone());
        }
        self.fonts()
            .entries()
            .first()
            .map(crate::fonts::FontEntry::reference)
    }

    /// What the tool would make of `words` now.
    fn text_sizes(&self, words: &str) -> Option<TextSizes> {
        let prefs = &self.prefs().text;
        Some(TextSizes {
            text: words.to_owned(),
            font: self.text_font()?,
            height_mm: prefs
                .height_mm
                .unwrap_or_else(|| default_height_mm(self.project().display_units)),
            letter_spacing_fraction: prefs.letter_spacing_fraction,
            line_spacing_fraction: prefs.line_spacing_fraction,
            align: prefs.align,
        })
    }

    /// Hands `event` to the Text tool.
    pub(super) fn text_event(&mut self, event: Event) -> Reply {
        let reply = match event {
            Event::Move(pointer) => {
                self.tool.pointer = Some(pointer);
                Reply::default()
            }
            Event::Leave => {
                self.tool.pointer = None;
                Reply::default()
            }
            // A click sets the start, and sets it again.
            Event::Press(pointer) => {
                self.tool.text.at = Some(self.snap_at(&pointer, None, &[]).at_mm);
                Reply::default()
            }
            Event::Text(words) => {
                self.tool.text.words = words;
                Reply::default()
            }
            Event::Field { field, text } => {
                self.type_text_size(field, &text);
                Reply::default()
            }
            Event::Option(ToolOption::Font(font)) => {
                self.prefs_mut().text.font = font;
                Reply::default()
            }
            Event::Option(ToolOption::TextAlign(align)) => {
                self.prefs_mut().text.align = align;
                Reply::default()
            }
            Event::Option(option) => {
                self.set_option(option);
                Reply::default()
            }
            Event::Key(Key::Enter) | Event::Done => return self.commit_text(),
            // Escape takes the click back; with none, it puts the tool
            // down.
            Event::Key(Key::Escape) => {
                if self.tool.text.at.is_none() {
                    self.arm(Tool::Select);
                    return Reply::default();
                }
                self.tool.text = TextDraft::default();
                Reply::default()
            }
            _ => Reply::default(),
        };
        self.refresh_text_draft();
        reply
    }

    /// A size typed in the bar: kept for the texts that follow, or
    /// refused at its field.
    fn type_text_size(&mut self, field: SizeField, typed: &str) {
        self.tool.problems.remove(&field);
        if typed.trim().is_empty() {
            return;
        }
        let value = match self.read_size(field, typed) {
            Ok(value) => value,
            Err(problem) => {
                self.tool.problems.insert(field, problem);
                return;
            }
        };
        let must_be_positive = matches!(field, SizeField::TextHeight | SizeField::LineSpacing);
        if must_be_positive && value <= 0.0 {
            let problem = self
                .strings()
                .get(StrKey::DrawProblemNotPositive)
                .to_owned();
            self.tool.problems.insert(field, problem);
            return;
        }
        let prefs = &mut self.prefs_mut().text;
        match field {
            SizeField::TextHeight => prefs.height_mm = Some(value),
            SizeField::LetterSpacing => prefs.letter_spacing_fraction = value,
            SizeField::LineSpacing => prefs.line_spacing_fraction = value,
            _ => {}
        }
    }

    /// The text as it would be made: its outlines where it starts.
    fn text_made(&self) -> Option<(TextSizes, dc_text::Laid, Point)> {
        let at = self.tool.text.at?;
        if !self.tool.problems.is_empty() || self.tool.text.words.trim().is_empty() {
            return None;
        }
        let sizes = self.text_sizes(&self.tool.text.words)?;
        let laid = text::outline(self.fonts(), &sizes, self.project().flatten_tolerance_mm).ok()?;
        (!laid.outlines.is_empty()).then_some((sizes, laid, at))
    }

    /// **Done**: the text made, selected, the tool ready for the next.
    fn commit_text(&mut self) -> Reply {
        let Some((sizes, laid, at)) = self.text_made() else {
            return Reply::default();
        };
        let name = next_name(self.project(), self.strings().get(StrKey::DrawNameText));
        let command = Command::AddShapes(vec![NewShape {
            name,
            shape: Some(sizes.shape()),
            profiles: laid.outlines,
            transform: Transform::translate(at.x, at.y),
            construction: self.tool.construction,
            folder: self.selection_folder(),
        }]);
        match self.run(command) {
            Ok(label) => {
                self.tool.text = TextDraft::default();
                self.tool.problems.clear();
                self.refresh_text_draft();
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

    /// The preview: the words where they would be made, or, before the
    /// click, where the click would put their start.
    fn refresh_text_draft(&mut self) {
        let mut draft = Draft::default();
        if let Some((_, laid, at)) = self.text_made() {
            let placed = Transform::translate(at.x, at.y);
            draft.outlines = laid
                .outlines
                .iter()
                .filter_map(|outline| placed.apply_polyline(outline).ok())
                .collect();
        }
        if self.tool.text.at.is_none()
            && let Some(pointer) = self.tool.pointer
        {
            draft.snap = Some(self.snap_at(&pointer, None, &[]));
        }
        self.tool.draft = draft;
    }

    /// The Text tool's bar.
    pub(super) fn text_bar(&self) -> ToolBar {
        let strings = self.strings();
        let units = self.project().display_units;
        let prefs = &self.prefs().text;
        let placed = self.tool.text.at.is_some();
        let made = self.text_made();
        let font = self.text_font().unwrap_or_else(|| prefs.font.clone());
        let prompt = if placed {
            StrKey::TextPromptType
        } else {
            StrKey::TextPromptPlace
        };
        ToolBar {
            tool: Tool::Draw(Drawing::Text),
            prompt: strings.get(prompt).to_owned(),
            fields: text::size_rows(
                strings,
                units,
                (
                    prefs.height_mm.unwrap_or_else(|| default_height_mm(units)),
                    prefs.letter_spacing_fraction,
                    prefs.line_spacing_fraction,
                ),
                |field| self.tool.problems.get(&field).cloned(),
            ),
            choices: vec![
                text::font_choice(strings, self.fonts(), &font),
                text::align_choice(strings, prefs.align),
            ],
            actions: Vec::new(),
            note: made
                .as_ref()
                .and_then(|(_, laid, _)| text::note(strings, &laid.warnings)),
            can_finish: made.is_some(),
            text: placed.then(|| text::text_row(strings, &self.tool.text.words)),
        }
    }
}
