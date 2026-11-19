// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reading a [`Bitmap`]'s stored picture (format 27;
//! `docs/specs/phase-3/design.md`, "Data model → dc-project" and "Bitmap
//! import and storage", steps 4 and 5; security review M4).
//!
//! A `.dcv` file is untrusted input, and a bitmap's `png_base64` is the one
//! field of it that is decompressed, so every reader here checks before it
//! allocates:
//!
//! - [`Bitmap::header`] decodes the PNG signature and `IHDR` chunk alone,
//!   the first 33 bytes of the picture, whatever its size. Loading and
//!   validating a project read nothing else of a picture, so a shared file
//!   costs nothing per pixel to open.
//! - [`Bitmap::pixels`] checks the bitmap's own `width_px` and `height_px`
//!   and then the header against [`MAX_BITMAP_SIDE_PX`], each other and
//!   8-bit grayscale before it decodes any image data; counts the pixels
//!   with `checked_mul`; and decodes under an explicit [`png::Limits`]
//!   with the text and colour-profile chunks skipped, the chunks a PNG
//!   compresses beside its image data, so nothing in a crafted picture is
//!   inflated but the image data the header was checked for.
//!
//! Placement is Y-up (ADR-0001): the pixels are row-major from the
//! picture's **top** row, which sits at `y_mm + height_mm`
//! ([`Bitmap::pixel_width_mm`], [`Bitmap::pixel_height_mm`]). Nothing here
//! flips.

use crate::model::Bitmap;
use crate::serialize::LoadError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use std::fmt;
use std::io::Cursor;

/// The largest picture a project may hold, per side, in pixels. It lives
/// here, not in `dc-import`, because a hand-edited `.dcv` reaches
/// [`Bitmap::pixels`] without passing an importer (security review M4).
pub const MAX_BITMAP_SIDE_PX: u32 = 8192;

/// The bytes of a PNG's signature and `IHDR` chunk: 8 of signature, then
/// the chunk's length (4), type (4), data (13) and CRC (4).
const PNG_HEADER_BYTES: usize = 33;

/// The base64 characters that hold [`PNG_HEADER_BYTES`]: whole groups of
/// four characters for three bytes.
const PNG_HEADER_BASE64_CHARS: usize = PNG_HEADER_BYTES.div_ceil(3) * 4;

/// How many base64 characters [`check_base64`] decodes at a time: whole
/// groups of four.
const BASE64_CHUNK_CHARS: usize = 4096;

/// The colour type of a stored PNG, from its header. A bitmap's must be
/// [`Self::Grayscale`] at 8 bits; the others are named so a message can
/// say what a hand-edited file holds instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitmapColour {
    /// One gray sample per pixel.
    Grayscale,
    /// A gray sample and an alpha sample.
    GrayscaleAlpha,
    /// Red, green and blue samples.
    Rgb,
    /// Red, green, blue and alpha samples.
    Rgba,
    /// An index into a palette.
    Indexed,
}

impl BitmapColour {
    /// The colour type in words, as messages name it.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Grayscale => "grayscale",
            Self::GrayscaleAlpha => "grayscale with alpha",
            Self::Rgb => "RGB",
            Self::Rgba => "RGB with alpha",
            Self::Indexed => "palette",
        }
    }
}

impl fmt::Display for BitmapColour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.describe())
    }
}

/// What a stored PNG's header says: its size, colour type and bit depth.
/// Read by [`Bitmap::header`] without decoding any image data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitmapHeader {
    /// The PNG's width, pixels.
    pub width_px: u32,
    /// The PNG's height, pixels.
    pub height_px: u32,
    /// The PNG's colour type.
    pub colour: BitmapColour,
    /// Bits per sample: 1, 2, 4, 8 or 16.
    pub bit_depth: u8,
}

impl BitmapHeader {
    /// Whether the PNG is 8-bit grayscale, the one form a bitmap stores.
    #[must_use]
    pub fn is_grayscale_8(&self) -> bool {
        self.colour == BitmapColour::Grayscale && self.bit_depth == 8
    }
}

/// A bitmap's decoded picture: 8-bit gray samples, row-major from the
/// picture's top row, 0 black to 255 white. `bytes` holds `width_px ×
/// height_px` samples; the sample of column `i`, row `j` is at
/// `j × width_px + i`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grayscale {
    /// Samples per row.
    pub width_px: u32,
    /// Rows.
    pub height_px: u32,
    /// The samples, top row first.
    pub bytes: Vec<u8>,
}

/// Why a bitmap's stored picture cannot be read: the `reason` of
/// [`LoadError::Bitmap`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BitmapError {
    /// `png_base64` is not base64 in the standard alphabet with padding.
    #[error("its png_base64 is not padded base64 in the standard alphabet: {message}")]
    Base64 {
        /// The decoder's description.
        message: String,
    },
    /// The bytes are not a PNG the decoder can read: a bad signature or
    /// header, a truncated or corrupt stream, or a chunk past the decoder's
    /// memory limit.
    #[error("its PNG cannot be decoded: {message}")]
    Png {
        /// The decoder's description.
        message: String,
    },
    /// `width_px` or `height_px` is 0.
    #[error("it is {width_px} × {height_px} px, and a picture has at least one pixel each way")]
    Empty {
        /// The width at fault, pixels.
        width_px: u32,
        /// The height at fault, pixels.
        height_px: u32,
    },
    /// A side, the bitmap's or its PNG's, is above [`MAX_BITMAP_SIDE_PX`].
    #[error("it is {width_px} × {height_px} px, and a side is at most {limit_px} px")]
    TooLarge {
        /// The width at fault, pixels.
        width_px: u32,
        /// The height at fault, pixels.
        height_px: u32,
        /// [`MAX_BITMAP_SIDE_PX`].
        limit_px: u32,
    },
    /// The PNG's size is not the bitmap's `width_px` × `height_px`.
    #[error(
        "its PNG is {png_width_px} × {png_height_px} px, not the {width_px} × {height_px} px of its width_px and height_px"
    )]
    SizeMismatch {
        /// The bitmap's `width_px`.
        width_px: u32,
        /// The bitmap's `height_px`.
        height_px: u32,
        /// The PNG's width, pixels.
        png_width_px: u32,
        /// The PNG's height, pixels.
        png_height_px: u32,
    },
    /// The PNG is not 8-bit grayscale.
    #[error("its PNG is {bit_depth}-bit {colour}, not 8-bit grayscale")]
    NotGrayscale8 {
        /// The PNG's colour type.
        colour: BitmapColour,
        /// The PNG's bits per sample.
        bit_depth: u8,
    },
}

impl Bitmap {
    /// The PNG header alone: dimensions, colour type, bit depth. Cheap:
    /// it decodes the first 33 bytes of the picture and no image data,
    /// whatever the picture's size. It is what validation reads.
    ///
    /// Fails with [`LoadError::Bitmap`] when those bytes are not base64 or
    /// not a PNG header. It does not compare the header with `width_px`
    /// and `height_px`, nor with [`MAX_BITMAP_SIDE_PX`]: validation and
    /// [`Self::pixels`] do.
    pub fn header(&self) -> Result<BitmapHeader, LoadError> {
        self.read_header().map_err(|reason| self.load_error(reason))
    }

    /// The pixels, row-major from the top row of the picture, 0 black to
    /// 255 white.
    ///
    /// Before decoding any image data it checks `width_px` and `height_px`
    /// (neither 0, neither above [`MAX_BITMAP_SIDE_PX`]) and the PNG
    /// header against them and against 8-bit grayscale. It then decodes
    /// under an explicit `png::Limits` of `MAX_BITMAP_SIDE_PX² + one row`
    /// bytes with text and colour-profile chunks ignored, into a buffer
    /// whose size is computed with `checked_mul` (security review M4). Every failure is
    /// a [`LoadError::Bitmap`]; nothing panics and nothing is allocated
    /// for a picture past the limit.
    ///
    /// Decodes on each call: the model holds no cache, so `Clone` stays
    /// cheap and correct. Only generation and the preview call it.
    pub fn pixels(&self) -> Result<Grayscale, LoadError> {
        self.read_pixels().map_err(|reason| self.load_error(reason))
    }

    /// Checks what [`crate::load`] checks of a stored picture: all of
    /// `png_base64` is base64, and it starts with a PNG header. No image
    /// data is decoded and nothing the size of the picture is allocated.
    pub(crate) fn check_stored(&self) -> Result<(), LoadError> {
        check_base64(&self.png_base64)
            .and_then(|()| self.read_header().map(|_| ()))
            .map_err(|reason| self.load_error(reason))
    }

    /// The width of one pixel on the stock, mm: `width_mm / width_px`.
    /// Pixel column `i` covers `x` from `x_mm + i × w` to `x_mm + (i + 1)
    /// × w`. Not finite when `width_px` is 0, which validation refuses.
    #[must_use]
    pub fn pixel_width_mm(&self) -> f64 {
        self.width_mm / f64::from(self.width_px)
    }

    /// The height of one pixel on the stock, mm: `height_mm / height_px`.
    /// Row 0 is the picture's top row, so pixel row `j` covers `y` from
    /// `y_mm + height_mm − (j + 1) × h` to `y_mm + height_mm − j × h`
    /// (Y-up). Not finite when `height_px` is 0, which validation refuses.
    #[must_use]
    pub fn pixel_height_mm(&self) -> f64 {
        self.height_mm / f64::from(self.height_px)
    }

    /// `reason` as this bitmap's [`LoadError`].
    fn load_error(&self, reason: BitmapError) -> LoadError {
        LoadError::Bitmap {
            id: self.id.clone(),
            reason,
        }
    }

    /// [`Self::header`], with the reason alone.
    pub(crate) fn read_header(&self) -> Result<BitmapHeader, BitmapError> {
        // The header is in the first 33 bytes, so in the first 44
        // characters: whole groups of four, which decode on their own.
        let text = self.png_base64.as_bytes();
        let prefix = text.get(..PNG_HEADER_BASE64_CHARS).unwrap_or(text);
        let bytes = STANDARD.decode(prefix).map_err(|e| BitmapError::Base64 {
            message: e.to_string(),
        })?;
        let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), decoder_limits());
        let info = decoder.read_header_info().map_err(png_error)?;
        Ok(header_of(info))
    }

    /// [`Self::pixels`], with the reason alone.
    fn read_pixels(&self) -> Result<Grayscale, BitmapError> {
        let (width_px, height_px) = (self.width_px, self.height_px);
        // The bitmap's own numbers first: they cost nothing to read.
        check_size(width_px, height_px)?;
        // Then the PNG's header, before any image data is decoded.
        let header = self.read_header()?;
        check_size(header.width_px, header.height_px)?;
        if (header.width_px, header.height_px) != (width_px, height_px) {
            return Err(BitmapError::SizeMismatch {
                width_px,
                height_px,
                png_width_px: header.width_px,
                png_height_px: header.height_px,
            });
        }
        if !header.is_grayscale_8() {
            return Err(BitmapError::NotGrayscale8 {
                colour: header.colour,
                bit_depth: header.bit_depth,
            });
        }
        // Both sides are at most `MAX_BITMAP_SIDE_PX` here, so the count
        // fits wherever 2²⁶ does; `checked_mul` holds it on a 16-bit
        // `usize` too.
        let too_large = || BitmapError::TooLarge {
            width_px,
            height_px,
            limit_px: MAX_BITMAP_SIDE_PX,
        };
        let count = usize::try_from(width_px)
            .ok()
            .zip(usize::try_from(height_px).ok())
            .and_then(|(w, h)| w.checked_mul(h))
            .ok_or_else(too_large)?;

        let png = STANDARD
            .decode(&self.png_base64)
            .map_err(|e| BitmapError::Base64 {
                message: e.to_string(),
            })?;
        let mut decoder = png::Decoder::new_with_limits(Cursor::new(png), decoder_limits());
        // The chunks a PNG compresses beside its image data. Skipped, a
        // small one that would inflate to gigabytes costs nothing; the
        // gray samples need neither a caption nor a colour profile.
        decoder.set_ignore_text_chunk(true);
        decoder.set_ignore_iccp_chunk(true);
        // The stored samples as they are: no expansion, no stripping.
        decoder.set_transformations(png::Transformations::IDENTITY);
        let mut reader = decoder.read_info().map_err(png_error)?;
        // The same bytes the header came from; the decoder's own reading
        // of them decides what it writes, so it is checked again.
        if header_of(reader.info()) != header || reader.output_buffer_size() != Some(count) {
            return Err(BitmapError::Png {
                message: "the image data is not one 8-bit grayscale frame of the header's size"
                    .to_owned(),
            });
        }
        // At most 8192² bytes. Reserved first, so a machine that cannot
        // spare them is told so and the process is not aborted.
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(count)
            .map_err(|_| BitmapError::Png {
                message: format!("there is no memory for its {width_px} × {height_px} pixels"),
            })?;
        bytes.resize(count, 0_u8);
        let frame = reader.next_frame(&mut bytes).map_err(png_error)?;
        // An animated PNG's first frame may be smaller than the picture.
        if (frame.width, frame.height) != (width_px, height_px) {
            return Err(BitmapError::Png {
                message: format!(
                    "its first frame is {} × {} px, not the picture's size",
                    frame.width, frame.height
                ),
            });
        }
        Ok(Grayscale {
            width_px,
            height_px,
            bytes,
        })
    }
}

/// Refuses a picture with a side of 0 or above [`MAX_BITMAP_SIDE_PX`].
fn check_size(width_px: u32, height_px: u32) -> Result<(), BitmapError> {
    if width_px == 0 || height_px == 0 {
        return Err(BitmapError::Empty {
            width_px,
            height_px,
        });
    }
    if width_px > MAX_BITMAP_SIDE_PX || height_px > MAX_BITMAP_SIDE_PX {
        return Err(BitmapError::TooLarge {
            width_px,
            height_px,
            limit_px: MAX_BITMAP_SIDE_PX,
        });
    }
    Ok(())
}

/// The most the PNG decoder may allocate for one picture, beside the
/// pixel buffer [`Bitmap::pixels`] hands it: `MAX_BITMAP_SIDE_PX² + one
/// row` bytes (security review M4), the size of the largest picture's
/// image data with its filter bytes. The decoder's own buffers are a row
/// or two; the rest bounds what it may hold of an ancillary chunk. With
/// `png` 0.18 nothing a `.dcv` can hold reaches it, since
/// [`Bitmap::pixels`] skips the chunks that inflate: it is the backstop
/// for a chunk a later release of the decoder buffers or inflates.
fn decoder_limits() -> png::Limits {
    // 8192² + 8192 is below 2²⁷, so it fits a 32-bit `usize`; a narrower
    // one saturates, which only lowers the limit.
    let side = usize::try_from(MAX_BITMAP_SIDE_PX).unwrap_or(usize::MAX);
    png::Limits {
        bytes: side.saturating_mul(side).saturating_add(side),
    }
}

/// The header fields of a PNG's `info`.
fn header_of(info: &png::Info<'_>) -> BitmapHeader {
    let colour = match info.color_type {
        png::ColorType::Grayscale => BitmapColour::Grayscale,
        png::ColorType::GrayscaleAlpha => BitmapColour::GrayscaleAlpha,
        png::ColorType::Rgb => BitmapColour::Rgb,
        png::ColorType::Rgba => BitmapColour::Rgba,
        png::ColorType::Indexed => BitmapColour::Indexed,
    };
    let bit_depth = match info.bit_depth {
        png::BitDepth::One => 1,
        png::BitDepth::Two => 2,
        png::BitDepth::Four => 4,
        png::BitDepth::Eight => 8,
        png::BitDepth::Sixteen => 16,
    };
    BitmapHeader {
        width_px: info.width,
        height_px: info.height,
        colour,
        bit_depth,
    }
}

/// A decoder error as a [`BitmapError`].
fn png_error(err: png::DecodingError) -> BitmapError {
    BitmapError::Png {
        message: err.to_string(),
    }
}

/// Checks that all of `text` is base64 in the standard alphabet with
/// padding, as `STANDARD.decode` would, without allocating the decoded
/// bytes: whole groups of four characters decode on their own, and
/// padding may only end the last one.
fn check_base64(text: &str) -> Result<(), BitmapError> {
    let mut buffer = [0_u8; BASE64_CHUNK_CHARS / 4 * 3];
    let mut chunks = text.as_bytes().chunks(BASE64_CHUNK_CHARS).peekable();
    let mut offset = 0_usize;
    while let Some(chunk) = chunks.next() {
        let last = chunks.peek().is_none();
        if !last && let Some(at) = chunk.iter().position(|b| *b == b'=') {
            return Err(BitmapError::Base64 {
                message: format!("padding at offset {}, before the end", offset + at),
            });
        }
        STANDARD
            .decode_slice(chunk, &mut buffer)
            .map_err(|e| BitmapError::Base64 {
                message: format!(
                    "{e} (in the {BASE64_CHUNK_CHARS} characters from offset {offset})"
                ),
            })?;
        offset += chunk.len();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
