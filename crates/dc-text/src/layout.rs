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
use skrifa::charmap::Charmap;
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::{GlyphMetrics, Metrics};
use skrifa::outline::{DrawSettings, OutlineGlyphCollection, OutlinePen};
use skrifa::raw::TableProvider as _;
use skrifa::raw::tables::gpos::{ExtensionSubtable, PairPos, PositionLookup};
use skrifa::raw::tables::kern::SubtableKind;
use skrifa::{FontRef, GlyphId, MetadataProvider as _, Tag};

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
    let face = Face::of(crate::font::face(bytes)?);
    Ok(outline_layout(&face, spec, flatten_tolerance_mm))
}

/// A font as a layout reads it: its glyphs, their outlines and advances
/// at the font's own units, and the lookups that kern.
struct Face<'a> {
    font: FontRef<'a>,
    charmap: Charmap<'a>,
    outlines: OutlineGlyphCollection<'a>,
    glyphs: GlyphMetrics<'a>,
    metrics: Metrics,
    kern_lookups: Vec<u16>,
}

impl<'a> Face<'a> {
    /// `font` at its default instance, unscaled.
    fn of(font: FontRef<'a>) -> Self {
        let (size, at) = (Size::unscaled(), LocationRef::default());
        Self {
            charmap: font.charmap(),
            outlines: font.outline_glyphs(),
            glyphs: font.glyph_metrics(size, at),
            metrics: font.metrics(size, at),
            kern_lookups: kern_lookups(&font),
            font,
        }
    }

    /// The font units that are one capital height: the top of `H`, else
    /// the font's `capHeight`, else its ascender, else its em.
    fn capital_units(&self) -> f64 {
        let top_of_h = self
            .charmap
            .map('H')
            .and_then(|glyph| self.glyphs.bounds(glyph))
            .map(|bounds| f64::from(bounds.y_max));
        [
            top_of_h,
            self.metrics.cap_height.map(f64::from),
            Some(f64::from(self.metrics.ascent)),
        ]
        .into_iter()
        .flatten()
        .find(|units| *units > 0.0)
        .unwrap_or_else(|| f64::from(self.metrics.units_per_em))
    }

    /// The kerning between `left` and `right`, font units: the `kern`
    /// table's when the font has one, else its GPOS pair adjustment.
    fn kerning(&self, left: GlyphId, right: GlyphId) -> f64 {
        if let Ok(table) = self.font.kern() {
            let pair = table
                .subtables()
                .flatten()
                .filter(|subtable| {
                    subtable.is_horizontal()
                        && !subtable.is_variable()
                        && !subtable.is_state_machine()
                })
                .find_map(|subtable| match subtable.kind().ok()? {
                    SubtableKind::Format0(pairs) => pairs.kerning(left, right),
                    SubtableKind::Format2(classes) => classes.kerning(left, right),
                    SubtableKind::Format3(classes) => classes.kerning(left, right),
                    SubtableKind::Format1(_) => None,
                });
            if let Some(pair) = pair {
                return f64::from(pair);
            }
        }
        let Ok(lookups) = self.font.gpos().and_then(|gpos| gpos.lookup_list()) else {
            return 0.0;
        };
        for index in &self.kern_lookups {
            let Ok(lookup) = lookups.lookups().get(usize::from(*index)) else {
                continue;
            };
            // A pair lookup, or one reached through an extension.
            let pairs: Vec<PairPos<'_>> = match lookup {
                PositionLookup::Pair(lookup) => lookup.subtables().iter().flatten().collect(),
                PositionLookup::Extension(lookup) => lookup
                    .subtables()
                    .iter()
                    .flatten()
                    .filter_map(|extension| match extension {
                        ExtensionSubtable::Pair(extension) => extension.extension().ok(),
                        _ => None,
                    })
                    .collect(),
                _ => continue,
            };
            // The first subtable that covers the pair decides it.
            if let Some(advance) = pairs
                .iter()
                .find_map(|pair| pair_advance(pair, left, right))
            {
                return advance;
            }
        }
        0.0
    }
}

/// What the pair-adjustment subtable `pair` adds to `left`'s advance
/// before `right`, font units; `None` when it does not cover the pair.
fn pair_advance(pair: &PairPos<'_>, left: GlyphId, right: GlyphId) -> Option<f64> {
    let advance = match pair {
        PairPos::Format1(table) => {
            let covered = table.coverage().ok()?.get(left)?;
            let set = table.pair_sets().get(usize::from(covered)).ok()?;
            let record = set
                .pair_value_records()
                .iter()
                .flatten()
                .find(|record| GlyphId::from(record.second_glyph()) == right)?;
            record.value_record1().x_advance()
        }
        PairPos::Format2(table) => {
            table.coverage().ok()?.get(left)?;
            let first = table.class_def1().ok()?.get(left);
            let second = table.class_def2().ok()?.get(right);
            let row = table.class1_records().get(usize::from(first)).ok()?;
            let cell = row.class2_records().get(usize::from(second)).ok()?;
            cell.value_record1().x_advance()
        }
    };
    Some(f64::from(advance.unwrap_or(0)))
}

/// The GPOS lookups of the font's `kern` features, each once, in the
/// table's order.
fn kern_lookups(font: &FontRef<'_>) -> Vec<u16> {
    let Ok(features) = font.gpos().and_then(|gpos| gpos.feature_list()) else {
        return Vec::new();
    };
    let kern = Tag::new(b"kern");
    let mut lookups: Vec<u16> = features
        .feature_records()
        .iter()
        .filter(|record| record.feature_tag() == kern)
        .filter_map(|record| record.feature(features.offset_data()).ok())
        .flat_map(|feature| {
            feature
                .lookup_list_indices()
                .iter()
                .map(|index| index.get())
                .collect::<Vec<u16>>()
        })
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

impl OutlinePen for Sink {
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
    scale: f64,
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
        let glyph = face.charmap.map(character).unwrap_or_else(|| {
            let warning = TextWarning::MissingGlyph { character };
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
            GlyphId::NOTDEF
        });
        if let Some(before) = before {
            pen += face.kerning(before, glyph) * scale;
        }
        sink.dx = pen;
        // A glyph with no outline (a space) draws nothing.
        if let Some(outline) = face.outlines.get(glyph) {
            let unscaled = DrawSettings::unhinted(Size::unscaled(), LocationRef::default());
            let _ = outline.draw(unscaled, &mut sink);
        }
        let advance = f64::from(face.glyphs.advance_width(glyph).unwrap_or(0.0)) * scale;
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
    let scale = spec.height_mm / face.capital_units();
    let mut warnings = Vec::new();
    let lines: Vec<(kurbo::BezPath, f64)> = spec
        .text
        .lines()
        .map(|text| line(face, text, spec, scale, &mut warnings))
        .collect();
    let widest = lines.iter().map(|(_, width)| *width).fold(0.0, f64::max);
    // The descent is below the baseline, and so negative.
    let line_units = f64::from(face.metrics.ascent) - f64::from(face.metrics.descent)
        + f64::from(face.metrics.leading);
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
