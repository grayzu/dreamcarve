// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A text's sizes (phase 5 functional spec, capabilities 84 and 91): the
//! words, the font, the height, the spacings and the alignment, as the
//! Properties panel shows and edits them, and the outline they give.
//!
//! [`dc_project::Shape::outline`] gives none for a text, since a project
//! does not carry its fonts (ADR-0006): the outline is laid out here,
//! with the fonts on hand ([`crate::fonts::Fonts`]), whenever a size
//! changes, and saved with the project. A text whose font is not on
//! this computer keeps the outline it was saved with; its sizes are
//! read-only and say why, until that font is added or another chosen.

use dc_geometry::Polyline;
use dc_project::{Align, FontRef, InstanceId, Project, Shape};
use dc_text::{Laid, TextSpec, TextWarning};

use crate::command::{Command, Edit, find_instance};
use crate::error::CommandError;
use crate::fonts::{self, Fonts};
use crate::shape::Section;
use crate::strings::{StrKey, Strings};
use crate::tool::{
    Choice, ChoiceOption, Quantity, SizeField, SizeRow, TextRow, ToolOption, size_text,
};
use crate::units;

/// What a text is made from: [`Shape::Text`]'s fields.
#[derive(Debug, Clone, PartialEq)]
pub struct TextSizes {
    /// The words; a line feed starts a line.
    pub text: String,
    /// The font.
    pub font: FontRef,
    /// The height of a capital letter, mm.
    pub height_mm: f64,
    /// Extra space between letters, a share of the height.
    pub letter_spacing_fraction: f64,
    /// The distance between lines, a share of the font's own.
    pub line_spacing_fraction: f64,
    /// How several lines are set.
    pub align: Align,
}

impl TextSizes {
    /// The sizes of `shape`, when it is a text.
    #[must_use]
    pub fn of(shape: &Shape) -> Option<Self> {
        let Shape::Text {
            text,
            font,
            height_mm,
            letter_spacing_fraction,
            line_spacing_fraction,
            align,
        } = shape
        else {
            return None;
        };
        Some(Self {
            text: text.clone(),
            font: font.clone(),
            height_mm: *height_mm,
            letter_spacing_fraction: *letter_spacing_fraction,
            line_spacing_fraction: *line_spacing_fraction,
            align: *align,
        })
    }

    /// These sizes as a shape.
    #[must_use]
    pub fn shape(&self) -> Shape {
        Shape::Text {
            text: self.text.clone(),
            font: self.font.clone(),
            height_mm: self.height_mm,
            letter_spacing_fraction: self.letter_spacing_fraction,
            line_spacing_fraction: self.line_spacing_fraction,
            align: self.align,
        }
    }
}

/// The outline of `sizes` in the fonts on hand, from the start of the
/// first line's baseline, with what the layout passed over.
///
/// # Errors
///
/// [`CommandError::FontMissing`] when the font is not on hand;
/// [`CommandError::Text`] for sizes the layout refuses.
pub fn outline(
    fonts: &Fonts,
    sizes: &TextSizes,
    flatten_tolerance_mm: f64,
) -> Result<Laid, CommandError> {
    let font = fonts
        .find(&sizes.font)
        .ok_or_else(|| CommandError::FontMissing {
            name: fonts::label(&sizes.font.family, &sizes.font.style),
        })?;
    let align = match sizes.align {
        Align::Left => dc_text::Align::Left,
        Align::Centre => dc_text::Align::Centre,
        Align::Right => dc_text::Align::Right,
    };
    Ok(dc_text::layout(
        font,
        &TextSpec {
            text: &sizes.text,
            height_mm: sizes.height_mm,
            letter_spacing_fraction: sizes.letter_spacing_fraction,
            line_spacing_fraction: sizes.line_spacing_fraction,
            align,
        },
        flatten_tolerance_mm,
    )?)
}

/// What the layout passed over, as the app says it: the first of it.
#[must_use]
pub fn note(strings: &Strings, warnings: &[TextWarning]) -> Option<String> {
    warnings.first().map(|warning| match warning {
        TextWarning::MissingGlyph { character } => strings.fmt(
            StrKey::TextNoteMissingGlyph,
            &[("character", &character.to_string())],
        ),
    })
}

/// The Font choice: the fonts on hand as a list, `chosen` the one in
/// use. A font that is not on hand is not in the list, and none is
/// chosen.
pub(crate) fn font_choice(strings: &Strings, fonts: &Fonts, chosen: &FontRef) -> Choice {
    let options: Vec<ChoiceOption> = fonts
        .entries()
        .iter()
        .map(|entry| ChoiceOption {
            label: entry.label(),
            option: ToolOption::Font(entry.reference()),
        })
        .collect();
    Choice {
        label: strings.get(StrKey::TextFont).to_owned(),
        help: strings.get(StrKey::TextFontHelp).to_owned(),
        selected: fonts
            .entries()
            .iter()
            .position(|entry| entry.reference() == *chosen)
            .unwrap_or(usize::MAX),
        options,
        list: true,
    }
}

/// The Align choice.
pub(crate) fn align_choice(strings: &Strings, chosen: Align) -> Choice {
    let options = [
        (StrKey::TextAlignLeft, Align::Left),
        (StrKey::TextAlignCentre, Align::Centre),
        (StrKey::TextAlignRight, Align::Right),
    ];
    Choice {
        label: strings.get(StrKey::TextAlign).to_owned(),
        help: strings.get(StrKey::TextAlignHelp).to_owned(),
        selected: options
            .iter()
            .position(|(_, align)| *align == chosen)
            .unwrap_or(0),
        options: options
            .into_iter()
            .map(|(label, align)| ChoiceOption {
                label: strings.get(label).to_owned(),
                option: ToolOption::TextAlign(align),
            })
            .collect(),
        list: false,
    }
}

/// The three size rows of a text: its height and its two spacings.
pub(crate) fn size_rows(
    strings: &Strings,
    units: dc_project::Units,
    sizes: (f64, f64, f64),
    problem: impl Fn(SizeField) -> Option<String>,
) -> Vec<SizeRow> {
    let (height_mm, letter, line) = sizes;
    [
        (SizeField::TextHeight, height_mm),
        (SizeField::LetterSpacing, letter),
        (SizeField::LineSpacing, line),
    ]
    .into_iter()
    .map(|(field, value)| SizeRow {
        field,
        label: strings.get(field.label()).to_owned(),
        help: strings.get(field.help()).to_owned(),
        text: match field.quantity() {
            Quantity::Length => units::field_text(value, units),
            _ => size_text(field, value, units),
        },
        quantity: field.quantity(),
        pinned: false,
        problem: problem(field),
        option: true,
    })
    .collect()
}

/// The words' row.
pub(crate) fn text_row(strings: &Strings, text: &str) -> TextRow {
    TextRow {
        label: strings.get(StrKey::TextWords).to_owned(),
        help: strings.get(StrKey::TextWordsHelp).to_owned(),
        text: text.to_owned(),
    }
}

/// A change to a text's sizes, as Properties makes it.
#[derive(Debug, Clone, PartialEq)]
pub enum TextEdit {
    /// The words.
    Text(String),
    /// The font.
    Font(FontRef),
    /// The height or a spacing, as typed.
    Size(SizeField, String),
    /// The alignment.
    Align(Align),
}

/// The sizes of instance `id`, when it is a text, with the instance.
fn sizes_of<'a>(
    project: &'a Project,
    id: &InstanceId,
) -> Option<(&'a dc_project::FeatureInstance, TextSizes)> {
    let instance = find_instance(project, id).ok()?;
    let shape = project
        .definitions
        .get(&instance.definition)?
        .shape
        .as_ref()?;
    Some((instance, TextSizes::of(shape)?))
}

/// The Shape section of Properties for instance `id` when it is a text:
/// its words, its font, its sizes and its alignment. Read-only, with the
/// reason as its note, when the text's font is not on hand, when it or
/// a linked copy of it is locked, or when it is stretched. `None` for
/// any other shape.
#[must_use]
pub fn section(
    project: &Project,
    strings: &Strings,
    fonts: &Fonts,
    id: &InstanceId,
) -> Option<Section> {
    let (instance, sizes) = sizes_of(project, id)?;
    let units = project.display_units;
    let missing = fonts.find(&sizes.font).is_none();
    let locked = project
        .instances
        .iter()
        .any(|other| other.definition == instance.definition && other.locked);
    // A text is laid out at its own size: it is shown as placed only
    // when the placement scales it evenly.
    let scale = crate::shape::even_scale(instance.transform);
    let note = if missing {
        Some(strings.fmt(
            StrKey::TextNoteFontMissing,
            &[("name", &fonts::label(&sizes.font.family, &sizes.font.style))],
        ))
    } else if scale.is_none() {
        Some(strings.get(StrKey::ShapeNoteStretched).to_owned())
    } else {
        None
    };
    Some(Section {
        title: strings.get(StrKey::DrawNameText).to_owned(),
        rows: size_rows(
            strings,
            units,
            (
                sizes.height_mm * scale.unwrap_or(1.0),
                sizes.letter_spacing_fraction,
                sizes.line_spacing_fraction,
            ),
            |_| None,
        ),
        choices: vec![
            font_choice(strings, fonts, &sizes.font),
            align_choice(strings, sizes.align),
        ],
        text: Some(text_row(strings, &sizes.text)),
        note,
        read_only: missing || locked || scale.is_none(),
    })
}

/// The command that makes `edit` to the text instance `id`: its sizes
/// changed and its outline laid out again in the fonts on hand, for
/// every linked copy, one undo step.
///
/// # Errors
///
/// [`CommandError::NothingToDo`] for another kind of shape, or a change
/// that changes nothing; [`CommandError::FontMissing`] when the font the
/// text would be laid out in is not on hand; [`CommandError::NotAQuantity`]
/// for a size that is none; [`CommandError::EmptyOutline`] for words
/// that draw nothing.
pub fn set(
    project: &Project,
    strings: &Strings,
    fonts: &Fonts,
    id: &InstanceId,
    edit: &TextEdit,
) -> Result<Command, CommandError> {
    let (instance, before) = sizes_of(project, id).ok_or(CommandError::NothingToDo)?;
    let units = project.display_units;
    let scale =
        crate::shape::even_scale(instance.transform).ok_or_else(|| CommandError::Stretched {
            name: instance.name.clone(),
        })?;
    let mut sizes = before.clone();
    let what = match edit {
        TextEdit::Text(text) => {
            sizes.text.clone_from(text);
            Edit::Text
        }
        TextEdit::Font(font) => {
            sizes.font = font.clone();
            Edit::Font
        }
        TextEdit::Align(align) => {
            sizes.align = *align;
            Edit::Align
        }
        TextEdit::Size(field, typed) => {
            let value =
                crate::shape::read_size(strings, units, *field, typed).map_err(|reason| {
                    CommandError::NotAQuantity {
                        text: typed.clone(),
                        reason,
                    }
                })?;
            let positive = || {
                if value > 0.0 {
                    Ok(())
                } else {
                    Err(CommandError::NotAQuantity {
                        text: typed.clone(),
                        reason: strings.get(StrKey::DrawProblemNotPositive).to_owned(),
                    })
                }
            };
            match field {
                SizeField::TextHeight => {
                    positive()?;
                    // Typed as the text is placed.
                    sizes.height_mm = value / scale;
                }
                SizeField::LetterSpacing => sizes.letter_spacing_fraction = value,
                SizeField::LineSpacing => {
                    positive()?;
                    sizes.line_spacing_fraction = value;
                }
                _ => return Err(CommandError::NothingToDo),
            }
            Edit::Size(*field)
        }
    };
    if sizes == before {
        return Err(CommandError::NothingToDo);
    }
    let laid = outline(fonts, &sizes, project.flatten_tolerance_mm)?;
    if laid.outlines.is_empty() {
        return Err(CommandError::EmptyOutline);
    }
    Ok(Command::SetOutline {
        instance: id.clone(),
        shape: Some(sizes.shape()),
        profiles: laid.outlines,
        what,
    })
}

/// The outlines `sizes` give, for a caller that only draws them.
#[must_use]
pub fn preview(fonts: &Fonts, sizes: &TextSizes, flatten_tolerance_mm: f64) -> Vec<Polyline> {
    outline(fonts, sizes, flatten_tolerance_mm)
        .map(|laid| laid.outlines)
        .unwrap_or_default()
}
