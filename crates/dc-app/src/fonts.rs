// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The fonts on hand (phase 5 functional spec, capability 91): the ones
//! that travel with the app, then the ones the user has added. The app
//! never lists the system's fonts, and this crate reads no file: the
//! shell reads the user's fonts folder and hands each file's bytes to
//! [`Fonts::add`].
//!
//! A project names a text's font by family and style
//! ([`dc_project::FontRef`]) and carries the outline, not the font. A
//! text whose font is not here keeps its outline and cannot be edited
//! until the font is added or another is chosen ([`crate::text`]).

use std::sync::Arc;

use dc_project::FontRef;
use dc_text::{Font, FontKind, TextError};

/// Where a font on hand came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSource {
    /// It travels with the app.
    Bundled,
    /// The user added it.
    User,
}

/// One font on hand.
#[derive(Debug, Clone)]
pub struct FontEntry {
    /// Its family, as the font names itself.
    pub family: String,
    /// Its style, as the font names itself.
    pub style: String,
    /// Outlines, or single strokes for Engrave.
    pub kind: FontKind,
    /// Bundled, or the user's.
    pub source: FontSource,
    font: Font,
}

impl FontEntry {
    /// The name the project knows it by.
    #[must_use]
    pub fn reference(&self) -> FontRef {
        FontRef {
            family: self.family.clone(),
            style: self.style.clone(),
        }
    }

    /// Its name in a list: the family, with the style when it is not
    /// the regular one (*Inter*, *Inter Bold*).
    #[must_use]
    pub fn label(&self) -> String {
        label(&self.family, &self.style)
    }
}

/// A font's name in a list and in a message: the family, with the style
/// when it is not the regular one.
#[must_use]
pub fn label(family: &str, style: &str) -> String {
    if style.is_empty() || style.eq_ignore_ascii_case("regular") {
        family.to_owned()
    } else {
        format!("{family} {style}")
    }
}

/// The fonts on hand, in the order the Font list offers them.
#[derive(Debug, Clone, Default)]
pub struct Fonts {
    entries: Vec<FontEntry>,
}

/// Two lists are the same when they name the same fonts in the same
/// order: a font is its family and style.
impl PartialEq for Fonts {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self
                .entries
                .iter()
                .zip(&other.entries)
                .all(|(a, b)| a.family == b.family && a.style == b.style && a.source == b.source)
    }
}

impl Fonts {
    /// The fonts that travel with the app ([`dc_text::bundled`]).
    #[must_use]
    pub fn bundled() -> Self {
        let entries = dc_text::bundled()
            .iter()
            .filter_map(|bundled| {
                let font = dc_text::bundled_font(bundled.family, bundled.style)?;
                Some(FontEntry {
                    family: bundled.family.to_owned(),
                    style: bundled.style.to_owned(),
                    kind: font.kind(),
                    source: FontSource::Bundled,
                    font,
                })
            })
            .collect();
        Self { entries }
    }

    /// Every font on hand.
    #[must_use]
    pub fn entries(&self) -> &[FontEntry] {
        &self.entries
    }

    /// The font a project names, when it is on hand.
    #[must_use]
    pub fn find(&self, font: &FontRef) -> Option<&Font> {
        self.entries
            .iter()
            .find(|entry| entry.family == font.family && entry.style == font.style)
            .map(|entry| &entry.font)
    }

    /// Adds the font file `bytes` as a font of the user's, after the
    /// fonts already here, and says what it is named. A font of the same
    /// family and style is already on hand: that one stands, and the
    /// file is not added twice.
    ///
    /// # Errors
    ///
    /// [`TextError`] when the bytes are no font the app can read.
    pub fn add(&mut self, bytes: Arc<[u8]>) -> Result<FontRef, TextError> {
        let font = Font::parse(bytes)?;
        let reference = FontRef {
            family: font.family().to_owned(),
            style: font.style().to_owned(),
        };
        if self.find(&reference).is_none() {
            self.entries.push(FontEntry {
                family: reference.family.clone(),
                style: reference.style.clone(),
                kind: font.kind(),
                source: FontSource::User,
                font,
            });
        }
        Ok(reference)
    }
}
