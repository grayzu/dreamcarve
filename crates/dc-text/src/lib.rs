// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Text for DreamCarve (phase 5 functional spec, capability 91;
//! `docs/specs/phase-5/design.md`, "dc-text (new crate)"): the fonts that
//! travel with the app, a font file read from its bytes, and the
//! outlines of text laid out in one.
//!
//! An outline font gives closed outlines, each outer anticlockwise and
//! followed by its counters, clockwise, in millimetres with Y up
//! (ADR-0001), so text is pocketed and carved as any drawn shape is. The
//! single-line font, *Simple Stroke*, gives open paths for Engrave.
//!
//! The crate reads no file and lists no system font: a caller hands it
//! the bytes ([`Font::parse`]), and the bundled fonts are compiled in
//! ([`bundled()`]). It never panics; what cannot be read or laid out is a
//! [`TextError`], and what was passed over a [`TextWarning`].

mod bundled;
mod font;
mod layout;
mod stroke;

pub use bundled::{Bundled, bundled, bundled_font};
pub use font::{Font, FontKind};
pub use layout::{Align, Laid, TextSpec, layout};

/// Why a font could not be read or text could not be laid out.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TextError {
    /// The bytes are no TrueType or OpenType font.
    #[error("this is not a font file the app can read: {reason}")]
    NotAFont {
        /// What the parser said.
        reason: String,
    },
    /// The font has no outlines to cut: a bitmap or colour font.
    #[error("this font has no outlines: a bitmap or colour font cannot be cut")]
    NoOutlines,
    /// The capital height asked is no size.
    #[error("the text height must be a size above 0, not {height_mm}")]
    BadHeight {
        /// The height asked, mm.
        height_mm: f64,
    },
    /// A spacing asked is no number.
    #[error("the letter and line spacing must be numbers")]
    BadSpacing,
}

/// Something the layout passed over.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TextWarning {
    /// The font has no glyph for a character of the text: its own
    /// missing-glyph mark is drawn in its place. Reported once a
    /// character.
    #[error("the font has no glyph for “{character}”")]
    MissingGlyph {
        /// The character.
        character: char,
    },
}
