// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The outlines of text laid out in a font (phase 5 design, "Text
//! layout"): lines of glyphs set from the origin, which is the start of
//! the first line's baseline, the lines below it going down the page.
//!
//! 1. **Scale.** The height asked is the height of a capital letter: the
//!    top of the font's `H`, else its `capHeight` (OS/2), else its
//!    ascender, becomes `height_mm`.
//! 2. **Glyphs.** Each character's glyph by `cmap`; one the font lacks
//!    is its `.notdef` and a [`TextWarning::MissingGlyph`]. The pen
//!    advances by `hmtx`, by the pair's kerning (the `kern` table, else
//!    GPOS's `kern` feature), and by `letter_spacing_fraction ·
//!    height_mm` between letters.
//! 3. **Outlines.** Each glyph's contours as quadratics and cubics,
//!    flattened within the tolerance (`dc_geometry::flatten`).
//! 4. **Lines.** One line down is `line_spacing_fraction · (ascender −
//!    descender + line gap)`; a line is shifted by `align` against the
//!    widest.
//! 5. **Winding.** A font fills by the non-zero rule, and so are the
//!    contours resolved (`dc_geometry::resolve_fill`, the SVG importer's):
//!    each outer anticlockwise and followed by its counters, clockwise,
//!    whatever the font's own winding, and letters that overlap (a
//!    script's joins, an accent on its base) made one outline.
//!
//! The single-line font skips steps 3 and 5 ([`crate::stroke`]).

use dc_geometry::{FillRule, Polyline};
use ttf_parser::gpos::{PairAdjustment, PositioningSubtable};
use ttf_parser::{Face, GlyphId, Tag};

use crate::font::Font;
use crate::{TextError, TextWarning};

/// How several lines are set against the widest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Every line starts at the origin's X.
    #[default]
    Left,
    /// Every line's middle is the widest line's.
    Centre,
    /// Every line ends where the widest does.
    Right,
}

/// What to lay out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSpec<'a> {
    /// The words; a line feed starts a line.
    pub text: &'a str,
    /// The height of a capital letter, mm; above 0.
    pub height_mm: f64,
    /// Extra space between letters, as a share of `height_mm`; 0 for
    /// the font's own spacing.
    pub letter_spacing_fraction: f64,
    /// The distance from one baseline to the next, as a share of the
    /// font's own; 1 for the font's own.
    pub line_spacing_fraction: f64,
    /// How several lines are set.
    pub align: Align,
}

/// Text laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Laid {
    /// The outlines, mm, Y-up, from the start of the first line's
    /// baseline: closed, each outer anticlockwise and followed by its
    /// counters, clockwise; open strokes for a single-line font. Empty
    /// for text with nothing to draw.
    pub outlines: Vec<Polyline>,
    /// The widest line's advance, mm: from its start to the end of its
    /// last glyph's advance.
    pub width_mm: f64,
    /// What was passed over.
    pub warnings: Vec<TextWarning>,
}

/// The outlines of `spec` set in `font`, no vertex further than
/// `flatten_tolerance_mm` from the font's curves.
///
/// # Errors
///
/// [`TextError::BadHeight`] and [`TextError::BadSpacing`] for sizes that
/// are none; [`TextError::NotAFont`] if the font's bytes no longer
/// parse, which [`Font::parse`] checked.
pub fn layout(
    font: &Font,
    spec: &TextSpec<'_>,
    flatten_tolerance_mm: f64,
) -> Result<Laid, TextError> {
    if !(spec.height_mm.is_finite() && spec.height_mm > 0.0) {
        return Err(TextError::BadHeight {
            height_mm: spec.height_mm,
        });
    }
    if !(spec.letter_spacing_fraction.is_finite() && spec.line_spacing_fraction.is_finite()) {
        return Err(TextError::BadSpacing);
    }
    let Some(bytes) = font.bytes() else {
        return Ok(crate::stroke::layout(spec));
    };
    let face = Face::parse(bytes, 0).map_err(|error| TextError::NotAFont {
        reason: error.to_string(),
    })?;
    Ok(outline_layout(&face, spec, flatten_tolerance_mm))
}

/// The font units that are one capital height: the top of `H`, else the
/// font's `capHeight`, else its ascender, else its em.
fn capital_units(face: &Face<'_>) -> f64 {
    let top_of_h = face
        .glyph_index('H')
        .and_then(|glyph| face.glyph_bounding_box(glyph))
        .map(|bounds| bounds.y_max);
    [top_of_h, face.capital_height(), Some(face.ascender())]
        .into_iter()
        .flatten()
        .find(|units| *units > 0)
        .map_or_else(|| f64::from(face.units_per_em()), f64::from)
}

/// The kerning between `left` and `right`, font units: the `kern`
/// table's when the font has one, else its GPOS pair adjustment.
fn kerning(face: &Face<'_>, kern_lookups: &[u16], left: GlyphId, right: GlyphId) -> f64 {
    if let Some(table) = face.tables().kern {
        let pair = table
            .subtables
            .into_iter()
            .filter(|subtable| {
                subtable.horizontal && !subtable.variable && !subtable.has_state_machine
            })
            .find_map(|subtable| subtable.glyphs_kerning(left, right));
        if let Some(pair) = pair {
            return f64::from(pair);
        }
    }
    let Some(gpos) = face.tables().gpos else {
        return 0.0;
    };
    for index in kern_lookups {
        let Some(lookup) = gpos.lookups.get(*index) else {
            continue;
        };
        for subtable in lookup.subtables.into_iter::<PositioningSubtable<'_>>() {
            let PositioningSubtable::Pair(pair) = subtable else {
                continue;
            };
            let adjusted = match pair {
                PairAdjustment::Format1 { coverage, sets } => coverage
                    .get(left)
                    .and_then(|covered| sets.get(covered))
                    .and_then(|set| set.get(right)),
                PairAdjustment::Format2 {
                    coverage,
                    classes,
                    matrix,
                } => coverage
                    .get(left)
                    .and_then(|_| matrix.get((classes.0.get(left), classes.1.get(right)))),
            };
            // The first subtable that covers the pair decides it.
            if let Some((first, _)) = adjusted {
                return f64::from(first.x_advance);
            }
        }
    }
    0.0
}

/// The GPOS lookups of the font's `kern` features, each once, in the
/// table's order.
fn kern_lookups(face: &Face<'_>) -> Vec<u16> {
    let Some(gpos) = face.tables().gpos else {
        return Vec::new();
    };
    let kern = Tag::from_bytes(b"kern");
    let mut lookups: Vec<u16> = gpos
        .features
        .into_iter()
        .filter(|feature| feature.tag == kern)
        .flat_map(|feature| feature.lookup_indices.into_iter())
        .collect();
    lookups.sort_unstable();
    lookups.dedup();
    lookups
}

/// A glyph's contours, written to a path at a scale and a place.
struct Sink {
    path: kurbo::BezPath,
    scale: f64,
    dx: f64,
}

impl Sink {
    fn at(&self, x: f32, y: f32) -> kurbo::Point {
        kurbo::Point::new(
            f64::from(x).mul_add(self.scale, self.dx),
            f64::from(y) * self.scale,
        )
    }
}

impl ttf_parser::OutlineBuilder for Sink {
    fn move_to(&mut self, x: f32, y: f32) {
        let to = self.at(x, y);
        self.path.move_to(to);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let to = self.at(x, y);
        self.path.line_to(to);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (control, to) = (self.at(x1, y1), self.at(x, y));
        self.path.quad_to(control, to);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (first, second, to) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.path.curve_to(first, second, to);
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

/// One line: its glyphs' contours from its start on its baseline, mm,
/// and its width.
fn line(
    face: &Face<'_>,
    text: &str,
    spec: &TextSpec<'_>,
    (scale, kern_lookups): (f64, &[u16]),
    warnings: &mut Vec<TextWarning>,
) -> (kurbo::BezPath, f64) {
    let gap = spec.letter_spacing_fraction * spec.height_mm;
    let mut sink = Sink {
        path: kurbo::BezPath::new(),
        scale,
        dx: 0.0,
    };
    let mut pen = 0.0;
    let mut width = 0.0;
    let mut before: Option<GlyphId> = None;
    for character in text.chars() {
        // A carriage return before a line feed, and the like, set
        // nothing.
        if character.is_control() {
            continue;
        }
        let glyph = face.glyph_index(character).unwrap_or_else(|| {
            let warning = TextWarning::MissingGlyph { character };
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
            GlyphId(0)
        });
        if let Some(before) = before {
            pen += kerning(face, kern_lookups, before, glyph) * scale;
        }
        sink.dx = pen;
        // A glyph with no outline (a space) draws nothing.
        let _ = face.outline_glyph(glyph, &mut sink);
        let advance = f64::from(face.glyph_hor_advance(glyph).unwrap_or(0)) * scale;
        // The gap follows a letter, and is no part of the line's width
        // after the last.
        width = pen + advance;
        pen = width + gap;
        before = Some(glyph);
    }
    (sink.path, width)
}

/// `spec` set in the outline font `face`.
fn outline_layout(face: &Face<'_>, spec: &TextSpec<'_>, flatten_tolerance_mm: f64) -> Laid {
    let scale = spec.height_mm / capital_units(face);
    let lookups = kern_lookups(face);
    let mut warnings = Vec::new();
    let lines: Vec<(kurbo::BezPath, f64)> = spec
        .text
        .lines()
        .map(|text| line(face, text, spec, (scale, &lookups), &mut warnings))
        .collect();
    let widest = lines.iter().map(|(_, width)| *width).fold(0.0, f64::max);
    let line_units =
        f64::from(face.ascender()) - f64::from(face.descender()) + f64::from(face.line_gap());
    let advance = spec.line_spacing_fraction * line_units * scale;
    let mut contours: Vec<Polyline> = Vec::new();
    let mut baseline = 0.0;
    for (mut path, width) in lines {
        let shift = match spec.align {
            Align::Left => 0.0,
            Align::Centre => (widest - width) / 2.0,
            Align::Right => widest - width,
        };
        path.apply_affine(kurbo::Affine::translate((shift, baseline)));
        contours.extend(
            dc_geometry::flatten(&path, flatten_tolerance_mm)
                .into_iter()
                .filter(Polyline::is_closed),
        );
        baseline -= advance;
    }
    let outlines = dc_geometry::resolve_fill(&contours, FillRule::NonZero, flatten_tolerance_mm)
        .into_iter()
        .flat_map(|region| {
            let holes: Vec<Polyline> = region
                .holes()
                .iter()
                .map(|hole| hole.polyline().clone())
                .collect();
            std::iter::once(region.outer().polyline().clone()).chain(holes)
        })
        .collect();
    Laid {
        outlines,
        width_mm: widest,
        warnings,
    }
}
