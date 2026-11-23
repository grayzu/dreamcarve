// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/text/index.ts

//! *Simple Stroke*, PureCutCNC's single-line font (functional spec,
//! capability 91): capitals, digits and `. - _ / ?`, each a few open
//! strokes for Engrave to follow (`GLYPHS`, `glyphFor`, `layoutGlyphs`).
//!
//! The table is upstream's, value for value: a glyph is drawn in a box
//! one capital height tall with Y running **down** from the capital line
//! (0) to the baseline (1). DreamCarve's Y runs up (ADR-0001), so a
//! point `(x, y)` of the table is laid at `(x, 1 − y)` heights from the
//! baseline. As upstream, a small letter is its capital, a character the
//! table lacks is `?`, a letter is followed by 0.18 of the height, and
//! lines are 1.45 heights apart. Its italic and condensed variants are
//! not ported.

use dc_geometry::{Polyline, Vertex};

use crate::TextWarning;
use crate::layout::{Align, Laid, TextSpec};

/// One glyph: its advance and its strokes, in capital heights.
struct Glyph {
    character: char,
    advance: f64,
    strokes: &'static [&'static [(f64, f64)]],
}

/// The space after every letter, in capital heights
/// (`LETTER_SPACING_RATIO`).
const LETTER_GAP: f64 = 0.18;
/// From one baseline to the next, in capital heights
/// (`LINE_HEIGHT_RATIO`).
const LINE_HEIGHT: f64 = 1.45;

/// Upstream's `GLYPHS`, in its order.
#[rustfmt::skip]
const GLYPHS: &[Glyph] = &[
    Glyph {
        character: 'A',
        advance: 1.0,
        strokes: &[&[(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)], &[(0.2, 0.58), (0.8, 0.58)]],
    },
    Glyph {
        character: 'B',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.0, 1.0)], &[(0.0, 0.0), (0.7, 0.0), (0.9, 0.18), (0.9, 0.38), (0.7, 0.5), (0.0, 0.5)], &[(0.0, 0.5), (0.72, 0.5), (0.92, 0.68), (0.92, 0.9), (0.72, 1.0), (0.0, 1.0)]],
    },
    Glyph {
        character: 'C',
        advance: 1.0,
        strokes: &[&[(0.92, 0.14), (0.7, 0.0), (0.2, 0.0), (0.0, 0.2), (0.0, 0.8), (0.2, 1.0), (0.7, 1.0), (0.92, 0.86)]],
    },
    Glyph {
        character: 'D',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.0, 1.0)], &[(0.0, 0.0), (0.6, 0.0), (0.94, 0.28), (0.94, 0.72), (0.6, 1.0), (0.0, 1.0)]],
    },
    Glyph {
        character: 'E',
        advance: 1.0,
        strokes: &[&[(0.95, 0.0), (0.0, 0.0), (0.0, 1.0), (0.95, 1.0)], &[(0.0, 0.5), (0.7, 0.5)]],
    },
    Glyph {
        character: 'F',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.0, 1.0)], &[(0.0, 0.0), (0.95, 0.0)], &[(0.0, 0.5), (0.7, 0.5)]],
    },
    Glyph {
        character: 'G',
        advance: 1.0,
        strokes: &[&[(0.92, 0.16), (0.72, 0.0), (0.2, 0.0), (0.0, 0.2), (0.0, 0.8), (0.2, 1.0), (0.72, 1.0), (0.92, 0.82), (0.92, 0.58), (0.56, 0.58)]],
    },
    Glyph {
        character: 'H',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.0, 1.0)], &[(1.0, 0.0), (1.0, 1.0)], &[(0.0, 0.5), (1.0, 0.5)]],
    },
    Glyph {
        character: 'I',
        advance: 0.72,
        strokes: &[&[(0.1, 0.0), (0.62, 0.0)], &[(0.36, 0.0), (0.36, 1.0)], &[(0.1, 1.0), (0.62, 1.0)]],
    },
    Glyph {
        character: 'J',
        advance: 1.0,
        strokes: &[&[(0.1, 0.0), (0.92, 0.0)], &[(0.72, 0.0), (0.72, 0.82), (0.56, 1.0), (0.22, 1.0), (0.06, 0.82)]],
    },
    Glyph {
        character: 'K',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.0, 1.0)], &[(0.96, 0.0), (0.0, 0.56), (1.0, 1.0)]],
    },
    Glyph {
        character: 'L',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.0, 1.0), (0.95, 1.0)]],
    },
    Glyph {
        character: 'M',
        advance: 1.2,
        strokes: &[&[(0.0, 1.0), (0.0, 0.0), (0.6, 0.58), (1.2, 0.0), (1.2, 1.0)]],
    },
    Glyph {
        character: 'N',
        advance: 1.08,
        strokes: &[&[(0.0, 1.0), (0.0, 0.0), (1.08, 1.0), (1.08, 0.0)]],
    },
    Glyph {
        character: 'O',
        advance: 1.0,
        strokes: &[&[(0.2, 0.0), (0.8, 0.0), (1.0, 0.2), (1.0, 0.8), (0.8, 1.0), (0.2, 1.0), (0.0, 0.8), (0.0, 0.2), (0.2, 0.0)]],
    },
    Glyph {
        character: 'P',
        advance: 1.0,
        strokes: &[&[(0.0, 1.0), (0.0, 0.0), (0.72, 0.0), (0.92, 0.18), (0.92, 0.42), (0.72, 0.58), (0.0, 0.58)]],
    },
    Glyph {
        character: 'Q',
        advance: 1.0,
        strokes: &[&[(0.2, 0.0), (0.8, 0.0), (1.0, 0.2), (1.0, 0.8), (0.8, 1.0), (0.2, 1.0), (0.0, 0.8), (0.0, 0.2), (0.2, 0.0)], &[(0.56, 0.68), (1.04, 1.08)]],
    },
    Glyph {
        character: 'R',
        advance: 1.0,
        strokes: &[&[(0.0, 1.0), (0.0, 0.0), (0.72, 0.0), (0.92, 0.18), (0.92, 0.42), (0.72, 0.58), (0.0, 0.58)], &[(0.42, 0.58), (1.0, 1.0)]],
    },
    Glyph {
        character: 'S',
        advance: 1.0,
        strokes: &[&[(0.92, 0.14), (0.72, 0.0), (0.2, 0.0), (0.0, 0.18), (0.0, 0.42), (0.2, 0.54), (0.72, 0.54), (0.92, 0.68), (0.92, 0.86), (0.72, 1.0), (0.2, 1.0), (0.0, 0.86)]],
    },
    Glyph {
        character: 'T',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (1.0, 0.0)], &[(0.5, 0.0), (0.5, 1.0)]],
    },
    Glyph {
        character: 'U',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.0, 0.8), (0.2, 1.0), (0.8, 1.0), (1.0, 0.8), (1.0, 0.0)]],
    },
    Glyph {
        character: 'V',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]],
    },
    Glyph {
        character: 'W',
        advance: 1.25,
        strokes: &[&[(0.0, 0.0), (0.24, 1.0), (0.62, 0.48), (1.0, 1.0), (1.25, 0.0)]],
    },
    Glyph {
        character: 'X',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (1.0, 1.0)], &[(1.0, 0.0), (0.0, 1.0)]],
    },
    Glyph {
        character: 'Y',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (0.5, 0.54), (1.0, 0.0)], &[(0.5, 0.54), (0.5, 1.0)]],
    },
    Glyph {
        character: 'Z',
        advance: 1.0,
        strokes: &[&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]],
    },
    Glyph {
        character: '0',
        advance: 1.0,
        strokes: &[&[(0.2, 0.0), (0.8, 0.0), (1.0, 0.2), (1.0, 0.8), (0.8, 1.0), (0.2, 1.0), (0.0, 0.8), (0.0, 0.2), (0.2, 0.0)], &[(0.2, 0.86), (0.8, 0.14)]],
    },
    Glyph {
        character: '1',
        advance: 0.72,
        strokes: &[&[(0.36, 0.0), (0.36, 1.0)], &[(0.12, 0.24), (0.36, 0.0), (0.58, 0.0)], &[(0.1, 1.0), (0.62, 1.0)]],
    },
    Glyph {
        character: '2',
        advance: 1.0,
        strokes: &[&[(0.08, 0.2), (0.28, 0.0), (0.76, 0.0), (0.96, 0.18), (0.96, 0.38), (0.06, 1.0), (0.96, 1.0)]],
    },
    Glyph {
        character: '3',
        advance: 1.0,
        strokes: &[&[(0.08, 0.14), (0.28, 0.0), (0.76, 0.0), (0.96, 0.18), (0.96, 0.4), (0.74, 0.52), (0.96, 0.64), (0.96, 0.86), (0.76, 1.0), (0.28, 1.0), (0.08, 0.86)]],
    },
    Glyph {
        character: '4',
        advance: 1.0,
        strokes: &[&[(0.82, 0.0), (0.82, 1.0)], &[(0.82, 0.0), (0.08, 0.62), (1.0, 0.62)]],
    },
    Glyph {
        character: '5',
        advance: 1.0,
        strokes: &[&[(0.92, 0.0), (0.18, 0.0), (0.08, 0.46), (0.72, 0.46), (0.92, 0.62), (0.92, 0.86), (0.72, 1.0), (0.2, 1.0), (0.04, 0.84)]],
    },
    Glyph {
        character: '6',
        advance: 1.0,
        strokes: &[&[(0.92, 0.14), (0.72, 0.0), (0.22, 0.0), (0.02, 0.22), (0.02, 0.8), (0.22, 1.0), (0.72, 1.0), (0.92, 0.82), (0.92, 0.58), (0.72, 0.42), (0.22, 0.42), (0.02, 0.58)]],
    },
    Glyph {
        character: '7',
        advance: 1.0,
        strokes: &[&[(0.04, 0.0), (0.96, 0.0), (0.36, 1.0)]],
    },
    Glyph {
        character: '8',
        advance: 1.0,
        strokes: &[&[(0.22, 0.0), (0.76, 0.0), (0.94, 0.16), (0.94, 0.36), (0.76, 0.5), (0.22, 0.5), (0.04, 0.36), (0.04, 0.16), (0.22, 0.0)], &[(0.22, 0.5), (0.76, 0.5), (0.96, 0.66), (0.96, 0.86), (0.76, 1.0), (0.22, 1.0), (0.04, 0.86), (0.04, 0.66), (0.22, 0.5)]],
    },
    Glyph {
        character: '9',
        advance: 1.0,
        strokes: &[&[(0.92, 0.42), (0.72, 0.58), (0.22, 0.58), (0.02, 0.42), (0.02, 0.2), (0.22, 0.0), (0.72, 0.0), (0.92, 0.2), (0.92, 0.78), (0.72, 1.0), (0.22, 1.0), (0.02, 0.84)]],
    },
    Glyph {
        character: '.',
        advance: 0.42,
        strokes: &[&[(0.2, 0.9), (0.22, 1.0)]],
    },
    Glyph {
        character: '-',
        advance: 0.72,
        strokes: &[&[(0.08, 0.5), (0.64, 0.5)]],
    },
    Glyph {
        character: '_',
        advance: 0.9,
        strokes: &[&[(0.04, 1.0), (0.86, 1.0)]],
    },
    Glyph {
        character: '/',
        advance: 0.9,
        strokes: &[&[(0.04, 1.0), (0.86, 0.0)]],
    },
    Glyph {
        character: ' ',
        advance: 0.6,
        strokes: &[],
    },
    Glyph {
        character: '?',
        advance: 1.0,
        strokes: &[&[(0.08, 0.18), (0.28, 0.0), (0.74, 0.0), (0.94, 0.18), (0.94, 0.34), (0.72, 0.5), (0.5, 0.58), (0.5, 0.74)], &[(0.5, 0.92), (0.5, 1.0)]],
    },
];

/// The glyph for `character`: its own, its capital's, or none.
fn glyph(character: char) -> Option<&'static Glyph> {
    let upper = character.to_uppercase().next().unwrap_or(character);
    GLYPHS.iter().find(|glyph| glyph.character == upper)
}

/// A stroke laid out: its points, mm.
type Stroke = Vec<(f64, f64)>;

/// One line's strokes, in millimetres from the line's start on its
/// baseline, and the line's width.
fn line(text: &str, spec: &TextSpec<'_>, warnings: &mut Vec<TextWarning>) -> (Vec<Stroke>, f64) {
    let height = spec.height_mm;
    let gap = (LETTER_GAP + spec.letter_spacing_fraction) * height;
    let mut strokes = Vec::new();
    let mut pen = 0.0;
    let mut width = 0.0;
    for character in text.chars() {
        let found = glyph(character).or_else(|| {
            let warning = TextWarning::MissingGlyph { character };
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
            glyph('?')
        });
        let Some(found) = found else {
            continue;
        };
        for stroke in found.strokes {
            strokes.push(
                stroke
                    .iter()
                    .map(|(x, y)| (x.mul_add(height, pen), (1.0 - y) * height))
                    .collect(),
            );
        }
        // The gap follows a letter, and is no part of the line's width
        // after the last.
        width = found.advance.mul_add(height, pen);
        pen = width + gap;
    }
    (strokes, width)
}

/// `spec` set in the single-line font: open paths.
pub(crate) fn layout(spec: &TextSpec<'_>) -> Laid {
    let mut warnings = Vec::new();
    let lines: Vec<(Vec<Stroke>, f64)> = spec
        .text
        .lines()
        .map(|text| line(text, spec, &mut warnings))
        .collect();
    let widest = lines.iter().map(|(_, width)| *width).fold(0.0, f64::max);
    let advance = LINE_HEIGHT * spec.line_spacing_fraction * spec.height_mm;
    let mut outlines = Vec::new();
    let mut baseline = 0.0;
    for (strokes, width) in lines {
        let shift = match spec.align {
            Align::Left => 0.0,
            Align::Centre => (widest - width) / 2.0,
            Align::Right => widest - width,
        };
        for stroke in strokes {
            let vertices: Vec<Vertex> = stroke
                .into_iter()
                .map(|(x, y)| Vertex::line(x + shift, y + baseline))
                .collect();
            if vertices.len() > 1 {
                outlines.push(Polyline::new(vertices, false));
            }
        }
        baseline -= advance;
    }
    Laid {
        outlines,
        width_mm: widest,
        warnings,
    }
}
