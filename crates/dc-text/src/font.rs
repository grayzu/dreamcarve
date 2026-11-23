// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A font: a TrueType or OpenType file's bytes, checked and named, or
//! the single-line font, which is a table of strokes
//! ([`crate::stroke`]).

use std::sync::Arc;

use crate::TextError;

/// The single-line font's family.
pub(crate) const SIMPLE_STROKE_FAMILY: &str = "Simple Stroke";
/// The single-line font's one style.
pub(crate) const SIMPLE_STROKE_STYLE: &str = "Regular";

/// What a font's glyphs are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    /// Closed outlines, counters as holes: pocketed, carved, cut out.
    Outline,
    /// Open strokes down the middle of each letter: engraved.
    SingleLine,
}

/// A font file's bytes: compiled in, or read by the caller.
#[derive(Debug, Clone)]
enum Bytes {
    Static(&'static [u8]),
    Shared(Arc<[u8]>),
}

impl Bytes {
    fn get(&self) -> &[u8] {
        match self {
            Self::Static(bytes) => bytes,
            Self::Shared(bytes) => bytes,
        }
    }
}

#[derive(Debug, Clone)]
enum Source {
    /// A font file, parsed again for each layout: parsing reads the
    /// table directory and no more.
    File(Bytes),
    /// The single-line font.
    SimpleStroke,
}

/// A font to lay text out in.
#[derive(Debug, Clone)]
pub struct Font {
    source: Source,
    family: String,
    style: String,
}

/// The name-table ids of the typographic family and subfamily, which a
/// font with more than four styles names itself by, and of the legacy
/// pair every font has.
const TYPOGRAPHIC_FAMILY: u16 = 16;
const TYPOGRAPHIC_SUBFAMILY: u16 = 17;
const FAMILY: u16 = 1;
const SUBFAMILY: u16 = 2;

/// The first name of `id` that reads as text.
fn name(face: &ttf_parser::Face<'_>, id: u16) -> Option<String> {
    face.names()
        .into_iter()
        .filter(|name| name.name_id == id)
        .find_map(|name| name.to_string())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

impl Font {
    /// Reads a `.ttf` or `.otf` font from its bytes: the first face of a
    /// collection, and a variable font at its default instance.
    ///
    /// # Errors
    ///
    /// [`TextError::NotAFont`] when the bytes are no font, with the
    /// parser's reason; [`TextError::NoOutlines`] for a font with no
    /// glyph outlines.
    pub fn parse(bytes: Arc<[u8]>) -> Result<Self, TextError> {
        Self::read(Bytes::Shared(bytes))
    }

    /// [`Font::parse`] for a font compiled into the program.
    pub(crate) fn of_static(bytes: &'static [u8]) -> Result<Self, TextError> {
        Self::read(Bytes::Static(bytes))
    }

    fn read(bytes: Bytes) -> Result<Self, TextError> {
        let face =
            ttf_parser::Face::parse(bytes.get(), 0).map_err(|error| TextError::NotAFont {
                reason: error.to_string(),
            })?;
        let tables = face.tables();
        if tables.glyf.is_none() && tables.cff.is_none() {
            return Err(TextError::NoOutlines);
        }
        let family = name(&face, TYPOGRAPHIC_FAMILY)
            .or_else(|| name(&face, FAMILY))
            .unwrap_or_default();
        let style = name(&face, TYPOGRAPHIC_SUBFAMILY)
            .or_else(|| name(&face, SUBFAMILY))
            .unwrap_or_else(|| SIMPLE_STROKE_STYLE.to_owned());
        Ok(Self {
            source: Source::File(bytes),
            family,
            style,
        })
    }

    /// The single-line font, *Simple Stroke*.
    #[must_use]
    pub fn simple_stroke() -> Self {
        Self {
            source: Source::SimpleStroke,
            family: SIMPLE_STROKE_FAMILY.to_owned(),
            style: SIMPLE_STROKE_STYLE.to_owned(),
        }
    }

    /// The family the font names itself: *Source Serif 4*.
    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    /// The style the font names itself: *Bold*.
    #[must_use]
    pub fn style(&self) -> &str {
        &self.style
    }

    /// Whether the font's glyphs are outlines or single strokes.
    #[must_use]
    pub const fn kind(&self) -> FontKind {
        match self.source {
            Source::File(_) => FontKind::Outline,
            Source::SimpleStroke => FontKind::SingleLine,
        }
    }

    /// The font file's bytes; `None` for the single-line font.
    pub(crate) fn bytes(&self) -> Option<&[u8]> {
        match &self.source {
            Source::File(bytes) => Some(bytes.get()),
            Source::SimpleStroke => None,
        }
    }
}
