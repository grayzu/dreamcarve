// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/text/outlineContours.test.ts

//! Slice 23 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Text"): the bundled fonts, the layout of outline text and the
//! single-line font. PureCutCNC lays out typeface.json fonts; what is
//! ported is what its tests ask of the result (counters as holes, the
//! stroke font's open paths), on real font files.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

use std::sync::Arc;

use dc_geometry::{Polyline, Rect};
use dc_text::{Align, Font, FontKind, Laid, TextError, TextSpec, TextWarning};

/// The engine's default flatten tolerance, mm.
const TOLERANCE_MM: f64 = 0.01;

fn font(family: &str, style: &str) -> Font {
    dc_text::bundled_font(family, style).unwrap_or_else(|| panic!("{family} {style} is bundled"))
}

fn spec(text: &str, height_mm: f64) -> TextSpec<'_> {
    TextSpec {
        text,
        height_mm,
        letter_spacing_fraction: 0.0,
        line_spacing_fraction: 1.0,
        align: Align::Left,
    }
}

fn laid(font: &Font, text: &str, height_mm: f64) -> Laid {
    dc_text::layout(font, &spec(text, height_mm), TOLERANCE_MM).unwrap()
}

fn bounds(outlines: &[Polyline]) -> Rect {
    let mut joint: Option<Rect> = None;
    for outline in outlines {
        let b = outline.bounds().unwrap();
        joint = Some(match joint {
            Some(a) => a.including(b.min).including(b.max),
            None => b,
        });
    }
    joint.expect("something was laid out")
}

/// The fonts that travel with the app, each with its licence, and the
/// single-line one last.
#[test]
fn every_bundled_font_parses() {
    let listed: Vec<(&str, &str, &str)> = dc_text::bundled()
        .iter()
        .map(|b| (b.family, b.style, b.licence))
        .collect();
    assert_eq!(
        listed,
        [
            ("Inter", "Regular", "OFL-1.1"),
            ("Inter", "Bold", "OFL-1.1"),
            ("Source Serif 4", "Regular", "OFL-1.1"),
            ("Source Serif 4", "Bold", "OFL-1.1"),
            ("Roboto Slab", "Bold", "Apache-2.0"),
            ("Pacifico", "Regular", "OFL-1.1"),
            ("Allerta Stencil", "Regular", "OFL-1.1"),
            ("Simple Stroke", "Regular", "Apache-2.0"),
        ]
    );
    for bundled in dc_text::bundled() {
        let font = font(bundled.family, bundled.style);
        // The names are the font file's own.
        assert_eq!(
            (font.family(), font.style()),
            (bundled.family, bundled.style)
        );
        let single = bundled.family == "Simple Stroke";
        assert_eq!(
            font.kind() == FontKind::SingleLine,
            single,
            "{}",
            bundled.family
        );
        let laid = laid(&font, "DREAMCARVE 2026", 10.0);
        assert!(!laid.outlines.is_empty(), "{}", bundled.family);
        assert!(
            laid.warnings.is_empty(),
            "{}: {:?}",
            bundled.family,
            laid.warnings
        );
        assert!(
            laid.outlines.iter().all(|o| o.is_closed() != single),
            "{}: closed outlines, or open strokes",
            bundled.family
        );
    }
    assert!(dc_text::bundled_font("Inter", "Black").is_none());
}

/// The height is the height of a capital letter, whatever the font: an
/// H of 12 mm is 12 mm tall and stands on the baseline.
#[test]
fn capital_height_is_the_asked_height() {
    for bundled in dc_text::bundled() {
        let font = font(bundled.family, bundled.style);
        let h = bounds(&laid(&font, "H", 12.0).outlines);
        assert!(
            (h.max.y - 12.0).abs() < 0.02 && h.min.y.abs() < 0.3,
            "{} {}: {h:?}",
            bundled.family,
            bundled.style
        );
        // Twice the height is twice the size.
        let twice = bounds(&laid(&font, "H", 24.0).outlines);
        assert!(
            (twice.width_mm() - 2.0 * h.width_mm()).abs() < 0.05,
            "{}",
            bundled.family
        );
    }
}

/// Port of the outline tests' counters: an outer is anticlockwise and is
/// followed by its counters, clockwise, whatever the font's own winding.
#[test]
fn counters_nest_as_holes() {
    for (family, style) in [("Inter", "Regular"), ("Source Serif 4", "Bold")] {
        let font = font(family, style);
        let o = laid(&font, "O", 20.0).outlines;
        assert_eq!(o.len(), 2, "{family}: an outer and a counter");
        assert!(o[0].signed_area_mm2() > 0.0 && o[1].signed_area_mm2() < 0.0);
        let (outer, counter) = (o[0].bounds().unwrap(), o[1].bounds().unwrap());
        assert!(outer.min.x < counter.min.x && counter.max.x < outer.max.x);

        let b = laid(&font, "B", 20.0).outlines;
        let areas: Vec<bool> = b.iter().map(|c| c.signed_area_mm2() > 0.0).collect();
        assert_eq!(areas, [true, false, false], "{family}: B has two counters");

        // Two letters: each outer with its own counter after it.
        let oo = laid(&font, "OO", 20.0).outlines;
        let areas: Vec<bool> = oo.iter().map(|c| c.signed_area_mm2() > 0.0).collect();
        assert_eq!(areas, [true, false, true, false], "{family}");
        // An i is two outers.
        let i = laid(&font, "i", 20.0).outlines;
        assert!(
            i.len() == 2 && i.iter().all(|c| c.signed_area_mm2() > 0.0),
            "{family}"
        );
    }
}

/// A script's letters run into one another: where they overlap they are
/// one outline, so nothing crosses and a pocket of the word is one area.
#[test]
fn overlapping_letters_are_one_outline() {
    let font = font("Pacifico", "Regular");
    let apart: usize = ["m", "i", "n"]
        .iter()
        .map(|letter| {
            laid(&font, letter, 20.0)
                .outlines
                .iter()
                .filter(|c| c.signed_area_mm2() > 0.0)
                .count()
        })
        .sum();
    let word = laid(&font, "min", 20.0).outlines;
    let joined = word.iter().filter(|c| c.signed_area_mm2() > 0.0).count();
    assert!(
        joined < apart,
        "{joined} outers for the word, {apart} for its letters"
    );
    // What is laid out nests: no outline crosses another.
    let contours = word
        .into_iter()
        .map(|c| dc_geometry::Contour::new(c).unwrap())
        .collect();
    assert!(dc_geometry::nest(contours).is_ok());
}

/// Letter spacing is extra space after each letter but the last, as a
/// share of the height; the font's own advance comes first.
#[test]
fn letter_spacing_adds_per_gap() {
    let font = font("Inter", "Regular");
    let plain = laid(&font, "HHH", 10.0);
    let one = laid(&font, "H", 10.0);
    assert!((plain.width_mm - 3.0 * one.width_mm).abs() < 1e-9);
    let spaced = dc_text::layout(
        &font,
        &TextSpec {
            letter_spacing_fraction: 0.5,
            ..spec("HHH", 10.0)
        },
        TOLERANCE_MM,
    )
    .unwrap();
    assert!((spaced.width_mm - plain.width_mm - 2.0 * 5.0).abs() < 1e-9);
    let (a, b) = (bounds(&plain.outlines), bounds(&spaced.outlines));
    assert!((b.max.x - a.max.x - 10.0).abs() < 0.02);
    // The single-line font's too.
    let stroke = font_stroke();
    let plain = laid(&stroke, "HH", 10.0).width_mm;
    let spaced = dc_text::layout(
        &stroke,
        &TextSpec {
            letter_spacing_fraction: 0.2,
            ..spec("HH", 10.0)
        },
        TOLERANCE_MM,
    )
    .unwrap()
    .width_mm;
    assert!((spaced - plain - 2.0).abs() < 1e-9);
}

fn font_stroke() -> Font {
    font("Simple Stroke", "Regular")
}

/// A font's kerning pairs are applied: AV is closer set than A and V
/// side by side, and a pair with none is not.
#[test]
fn kerning_pairs_apply() {
    for (family, style) in [("Inter", "Regular"), ("Source Serif 4", "Regular")] {
        let font = font(family, style);
        let width = |text: &str| laid(&font, text, 20.0).width_mm;
        assert!(
            width("AV") < width("A") + width("V") - 0.1,
            "{family}: AV {} against {}",
            width("AV"),
            width("A") + width("V")
        );
        assert!((width("HH") - 2.0 * width("H")).abs() < 1e-9, "{family}");
    }
}

/// Lines go down the page by the font's line height times the line
/// spacing, and are set left, centred or right against the widest.
#[test]
fn lines_align() {
    let font = font("Inter", "Bold");
    let line = |align: Align, spacing: f64| {
        dc_text::layout(
            &font,
            &TextSpec {
                align,
                line_spacing_fraction: spacing,
                ..spec("HHHHH\nH", 10.0)
            },
            TOLERANCE_MM,
        )
        .unwrap()
    };
    // The short line's H: the outline lowest on the page.
    let second = |laid: &Laid| -> Rect {
        laid.outlines
            .iter()
            .map(|o| o.bounds().unwrap())
            .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
            .unwrap()
    };
    let left = line(Align::Left, 1.0);
    let wide = bounds(&left.outlines);
    let one = bounds(&laid(&font, "H", 10.0).outlines);
    let h = second(&left);
    assert!((h.min.x - one.min.x).abs() < 0.02, "left: {h:?}");
    assert!(
        h.max.y < 0.0,
        "the second line is under the first's baseline"
    );
    let centred = second(&line(Align::Centre, 1.0));
    let middle = f64::midpoint(centred.min.x, centred.max.x);
    assert!(
        (middle - left.width_mm / 2.0).abs() < 0.02,
        "centre: {middle}"
    );
    let right = second(&line(Align::Right, 1.0));
    assert!((wide.max.x - right.max.x).abs() < 0.02, "right: {right:?}");
    assert_eq!(left.width_mm, line(Align::Right, 1.0).width_mm);

    // Twice the line spacing: the second line twice as far down.
    let (near, far) = (second(&left).min.y, second(&line(Align::Left, 2.0)).min.y);
    assert!((far - 2.0 * near).abs() < 0.02, "{near} and {far}");
    // A line with nothing on it still takes its height.
    let gap = bounds(&laid(&font, "H\n\nH", 10.0).outlines);
    assert!((gap.min.y - far).abs() < 0.02);
}

/// The same text, font and sizes give the same outlines, vertex for
/// vertex.
#[test]
fn layout_is_deterministic() {
    for bundled in dc_text::bundled() {
        let font = font(bundled.family, bundled.style);
        let first = laid(&font, "Hook plate No. 7", 8.0);
        assert_eq!(
            first,
            laid(&font, "Hook plate No. 7", 8.0),
            "{}",
            bundled.family
        );
    }
}

/// A file that is not a font is refused with the reason; a font file's
/// names are read from it.
#[test]
fn a_file_that_is_not_a_font_is_refused() {
    for bytes in [
        &b""[..],
        b"not a font at all",
        b"\x89PNG\r\n\x1a\n0000000000000000",
    ] {
        let refused = Font::parse(Arc::from(bytes)).unwrap_err();
        assert!(matches!(refused, TextError::NotAFont { .. }), "{refused:?}");
        assert!(
            refused
                .to_string()
                .starts_with("this is not a font file the app can read")
        );
    }
    let inter = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fonts/inter/Inter-Bold.ttf"
    ))
    .unwrap();
    let font = Font::parse(Arc::from(inter)).unwrap();
    assert_eq!((font.family(), font.style()), ("Inter", "Bold"));
    assert_eq!(font.kind(), FontKind::Outline);
}

/// A character the font has no glyph for is noted once and drawn as the
/// font's own missing-glyph mark; the rest of the text is laid out.
#[test]
fn a_missing_glyph_is_noted() {
    let font = font("Allerta Stencil", "Regular");
    let laid = laid(&font, "A\u{4e2d}\u{4e2d}B", 10.0);
    assert_eq!(
        laid.warnings,
        [TextWarning::MissingGlyph {
            character: '\u{4e2d}'
        }]
    );
    assert_eq!(
        laid.warnings[0].to_string(),
        "the font has no glyph for “\u{4e2d}”"
    );
    assert!(laid.width_mm > laid_width(&font, "AB"));
}

fn laid_width(font: &Font, text: &str) -> f64 {
    laid(font, text, 10.0).width_mm
}

/// Port of the stroke font's coverage: capitals, digits and `. - _ / ?`
/// are open paths within the capital height; a small letter is its
/// capital; anything else is `?`, and noted.
#[test]
fn simple_stroke_is_open_paths() {
    let font = font_stroke();
    for character in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-_/?".chars() {
        let text = character.to_string();
        let laid = laid(&font, &text, 10.0);
        assert!(laid.warnings.is_empty(), "{character}");
        assert!(!laid.outlines.is_empty(), "{character}");
        assert!(laid.outlines.iter().all(|o| !o.is_closed()), "{character}");
        let b = bounds(&laid.outlines);
        // Q's tail alone reaches under the baseline.
        let low = if character == 'Q' { -0.81 } else { -1e-9 };
        assert!(
            b.min.y >= low && b.max.y <= 10.0 + 1e-9,
            "{character}: {b:?}"
        );
        assert!(
            b.min.x >= -1e-9 && b.max.x <= laid.width_mm + 0.5,
            "{character}: {b:?}"
        );
    }
    // A stands on the baseline with its point at the capital height:
    // upstream's Y runs down the page, ours up.
    let a = laid(&font, "A", 10.0).outlines;
    let apex = a[0].vertices()[1];
    assert_eq!((apex.x, apex.y), (5.0, 10.0));
    assert_eq!(a[0].vertices()[0].y, 0.0);

    assert_eq!(laid(&font, "abc", 10.0), laid(&font, "ABC", 10.0));
    let unknown = laid(&font, "A#", 10.0);
    assert_eq!(
        unknown.warnings,
        [TextWarning::MissingGlyph { character: '#' }]
    );
    assert_eq!(unknown.outlines, laid(&font, "A?", 10.0).outlines);
    // A space draws nothing and takes its room; an advance is the
    // glyph's width and upstream's 0.18 of the height after it.
    assert!(laid(&font, " ", 10.0).outlines.is_empty());
    assert!((laid(&font, "H", 10.0).width_mm - 10.0).abs() < 1e-9);
    assert!((laid(&font, "HH", 10.0).width_mm - 21.8).abs() < 1e-9);
    // Lines are 1.45 heights apart.
    let two = bounds(&laid(&font, "H\nH", 10.0).outlines);
    assert!((two.min.y + 14.5).abs() < 1e-9);
}

/// A height that is no size, and nothing to set, are told apart from a
/// font that fails.
#[test]
fn a_bad_height_is_refused_and_no_text_is_nothing() {
    let font = font("Inter", "Regular");
    for height in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            dc_text::layout(&font, &spec("H", height), TOLERANCE_MM),
            Err(TextError::BadHeight { .. })
        ));
    }
    for text in ["", "  ", "\n"] {
        let nothing = laid(&font, text, 10.0);
        assert!(
            nothing.outlines.is_empty() && nothing.warnings.is_empty(),
            "{text:?}"
        );
    }
}
