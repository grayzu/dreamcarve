// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The one conversion of a pixel to the 8-bit gray a bitmap stores
//! (`docs/specs/phase-3/design.md`, "Bitmap import and storage", step 2).
//! Both decoders call [`Converter::pixel`] for every pixel that is not
//! already gray, so a PNG and a JPEG of the same colours read alike.

/// The denominator of [`gray`]: luminance weights in thousandths, times
/// the 255 of an alpha sample.
const DENOMINATOR: u32 = 1000 * 255;

/// The gray of the pixel `(r, g, b)` with alpha `a`, composited on white
/// and then weighted by Rec. 601 luminance:
///
/// ```text
/// c' = (a·c + (255 − a)·255) / 255          for c in R, G, B
/// Y  = round(0.299 R' + 0.587 G' + 0.114 B')
/// ```
///
/// computed in integers as one exact fraction,
/// `((299 R + 587 G + 114 B)·a + 1000·255·(255 − a)) / (1000·255)`, rounded
/// half up, so the result does not depend on floating point and is rounded
/// once. An opaque gray pixel (`a = 255`, `R = G = B`) gives its own value.
#[must_use]
pub(super) fn gray(r: u8, g: u8, b: u8, a: u8) -> u8 {
    let (r, g, b, a) = (u32::from(r), u32::from(g), u32::from(b), u32::from(a));
    let weighted = 299 * r + 587 * g + 114 * b;
    // At most 255 000 · 255, well inside a u32, doubled too.
    let numerator = weighted * a + DENOMINATOR * (255 - a);
    let rounded = (2 * numerator + DENOMINATOR) / (2 * DENOMINATOR);
    // `numerator ≤ 255 · DENOMINATOR`, so `rounded ≤ 255`.
    u8::try_from(rounded).unwrap_or(u8::MAX)
}

/// Converts pixels with [`gray`] and remembers what it found, for the
/// warnings: colour (a pixel that is not wholly transparent and whose
/// channels differ) and transparency (a pixel whose alpha is below 255).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Converter {
    /// Some pixel had colour.
    pub colour: bool,
    /// Some pixel was not opaque.
    pub alpha: bool,
}

impl Converter {
    /// [`gray`] of the pixel, noting its colour and alpha.
    pub(super) fn pixel(&mut self, r: u8, g: u8, b: u8, a: u8) -> u8 {
        if a > 0 && (r != g || g != b) {
            self.colour = true;
        }
        if a < u8::MAX {
            self.alpha = true;
        }
        gray(r, g, b, a)
    }

    /// Converts one row of 8-bit samples, `channels` per pixel (1 gray, 2
    /// gray and alpha, 3 RGB, 4 RGBA), into `out`, one byte per pixel.
    pub(super) fn row(&mut self, samples: &[u8], channels: usize, out: &mut [u8]) {
        let pixels = samples.chunks_exact(channels.max(1));
        for (pixel, gray_out) in pixels.zip(out.iter_mut()) {
            *gray_out = match *pixel {
                [v] => v,
                [v, a] => self.pixel(v, v, v, a),
                [r, g, b] => self.pixel(r, g, b, u8::MAX),
                [r, g, b, a] => self.pixel(r, g, b, a),
                _ => 0,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The formula, against the floating-point statement of it, at every
    /// alpha for a spread of colours: equal wherever the exact value is
    /// not within a hair of a half.
    #[test]
    fn gray_is_rec_601_on_white() {
        assert_eq!(gray(255, 0, 0, 255), 76);
        assert_eq!(gray(0, 255, 0, 255), 150);
        assert_eq!(gray(0, 0, 255, 255), 29);
        assert_eq!(gray(0, 0, 0, 0), 255);
        assert_eq!(gray(0, 0, 0, 255), 0);
        assert_eq!(gray(255, 255, 255, 255), 255);
        for v in 0..=255 {
            assert_eq!(gray(v, v, v, 255), v);
        }
        for a in 0..=255_u8 {
            for (r, g, b) in [(200, 60, 40), (40, 160, 80), (13, 7, 250), (0, 0, 0)] {
                let on_white =
                    |c: u8| (f64::from(a) * f64::from(c) + f64::from(255 - a) * 255.0) / 255.0;
                let y = 0.299 * on_white(r) + 0.587 * on_white(g) + 0.114 * on_white(b);
                if (y.fract() - 0.5).abs() > 1e-6 {
                    assert_eq!(f64::from(gray(r, g, b, a)), y.round(), "{r} {g} {b} {a}");
                }
            }
        }
    }
}
