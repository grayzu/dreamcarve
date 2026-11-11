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
mod tests {
    use super::*;
    use crate::ids::BitmapId;
    use crate::test_support::{SAMPLE_BITMAP_BYTES, gray_png_base64, png_base64, sample_bitmap};

    /// The PNG signature.
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

    /// The CRC-32 a PNG chunk ends with (ISO 3309, as the PNG
    /// specification's sample code computes it), bit by bit.
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    /// One PNG chunk: length, type, data, CRC.
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[4..]);
        out.extend_from_slice(&crc.to_be_bytes());
        out
    }

    /// An `IHDR` chunk that says whatever the test wants: `colour_type` 0
    /// is grayscale, 2 RGB; no interlace.
    fn ihdr(width_px: u32, height_px: u32, bit_depth: u8, colour_type: u8) -> Vec<u8> {
        let mut data = width_px.to_be_bytes().to_vec();
        data.extend_from_slice(&height_px.to_be_bytes());
        data.extend_from_slice(&[bit_depth, colour_type, 0, 0, 0]);
        chunk(b"IHDR", &data)
    }

    /// The chunks of `png` after its signature, as (type, data).
    fn chunks_of(png: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let mut rest = &png[SIGNATURE.len()..];
        let mut chunks = Vec::new();
        while !rest.is_empty() {
            let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
            let kind: [u8; 4] = rest[4..8].try_into().unwrap();
            chunks.push((kind, rest[8..8 + len].to_vec()));
            rest = &rest[12 + len..];
        }
        chunks
    }

    /// The bytes a `png_base64` holds.
    fn decoded(text: &str) -> Vec<u8> {
        STANDARD.decode(text).unwrap()
    }

    /// The `IDAT` data of `png`, joined: its zlib stream.
    fn image_data(png: &[u8]) -> Vec<u8> {
        chunks_of(png)
            .into_iter()
            .filter(|(kind, _)| kind == b"IDAT")
            .flat_map(|(_, data)| data)
            .collect()
    }

    /// A PNG of a signature and `chunks`, as a bitmap stores it.
    fn stored(chunks: &[Vec<u8>]) -> String {
        let mut png = SIGNATURE.to_vec();
        for chunk in chunks {
            png.extend_from_slice(chunk);
        }
        STANDARD.encode(png)
    }

    /// The sample bitmap holding `png_base64` and saying it is `width_px`
    /// × `height_px`.
    fn bitmap(width_px: u32, height_px: u32, png_base64: String) -> Bitmap {
        Bitmap {
            width_px,
            height_px,
            png_base64,
            ..sample_bitmap()
        }
    }

    /// The reason `result` failed for, which must be bitmap `b1`'s.
    fn reason<T: std::fmt::Debug>(result: Result<T, LoadError>) -> BitmapError {
        match result {
            Err(LoadError::Bitmap { id, reason }) => {
                assert_eq!(id, BitmapId::new("b1"));
                reason
            }
            other => panic!("expected LoadError::Bitmap, got {other:?}"),
        }
    }

    /// The zlib stream of `rows` rows of 8192 zero samples, a filter byte
    /// before each: what an 8192-wide all-black picture's `IDAT` holds.
    /// Tens of megabytes of zeros in some tens of kilobytes.
    fn zeros_stream(rows: u32) -> Vec<u8> {
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, MAX_BITMAP_SIDE_PX, rows);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let mut stream = writer.stream_writer().unwrap();
        let row = vec![0_u8; MAX_BITMAP_SIDE_PX as usize];
        for _ in 0..rows {
            std::io::Write::write_all(&mut stream, &row).unwrap();
        }
        stream.finish().unwrap();
        drop(writer);
        image_data(&png)
    }

    /// Design spec, "Bitmap import and storage", steps 4 and 5: the
    /// pixels are the stored PNG's samples, row-major from the picture's
    /// top row, and the header is read without them. The sample is 4 × 2
    /// pixels on 40 × 20 mm, so a pixel is 10 mm square.
    #[test]
    fn pixels_decode_the_stored_png() {
        let bitmap = sample_bitmap();
        assert_eq!(
            bitmap.header().unwrap(),
            BitmapHeader {
                width_px: 4,
                height_px: 2,
                colour: BitmapColour::Grayscale,
                bit_depth: 8,
            }
        );
        assert!(bitmap.header().unwrap().is_grayscale_8());
        let pixels = bitmap.pixels().unwrap();
        assert_eq!(
            pixels,
            Grayscale {
                width_px: 4,
                height_px: 2,
                bytes: SAMPLE_BITMAP_BYTES.to_vec(),
            }
        );
        // Row 0 is the top row: the ramp to white, not the darker one.
        assert_eq!(pixels.bytes[..4], [0, 85, 170, 255]);
        assert!((bitmap.pixel_width_mm() - 10.0).abs() < 1e-12);
        assert!((bitmap.pixel_height_mm() - 10.0).abs() < 1e-12);
        // No cache: a second call decodes again and gives the same.
        assert_eq!(bitmap.pixels().unwrap(), pixels);
        assert_eq!(bitmap.check_stored(), Ok(()));
    }

    /// A PNG whose size is not the bitmap's `width_px` × `height_px` is
    /// refused before its image data is decoded; its header still reads.
    #[test]
    fn wrong_size_png_is_an_error() {
        let png = gray_png_base64(3, 2, &[0, 1, 2, 3, 4, 5]);
        for (width_px, height_px) in [(2, 3), (3, 3), (4, 2), (6, 1)] {
            let bitmap = bitmap(width_px, height_px, png.clone());
            assert_eq!(
                reason(bitmap.pixels()),
                BitmapError::SizeMismatch {
                    width_px,
                    height_px,
                    png_width_px: 3,
                    png_height_px: 2,
                },
                "{width_px} × {height_px}"
            );
            let header = bitmap.header().unwrap();
            assert_eq!((header.width_px, header.height_px), (3, 2));
        }
        let err = bitmap(2, 3, png.clone()).pixels().unwrap_err().to_string();
        assert_eq!(
            err,
            "bitmap `b1` cannot be read: its PNG is 3 × 2 px, not the 2 × 3 px of its width_px and height_px"
        );
        assert!(bitmap(3, 2, png).pixels().is_ok());
    }

    /// A bitmap stores 8-bit grayscale and nothing else: a colour PNG, a
    /// 16-bit one, one with alpha or a palette is refused from its header.
    #[test]
    fn colour_png_is_an_error() {
        use png::{BitDepth, ColorType};
        let cases = [
            (ColorType::Rgb, BitDepth::Eight, 3, BitmapColour::Rgb, 8),
            (ColorType::Rgba, BitDepth::Eight, 4, BitmapColour::Rgba, 8),
            (
                ColorType::GrayscaleAlpha,
                BitDepth::Eight,
                2,
                BitmapColour::GrayscaleAlpha,
                8,
            ),
            (
                ColorType::Grayscale,
                BitDepth::Sixteen,
                2,
                BitmapColour::Grayscale,
                16,
            ),
        ];
        for (colour_type, depth, bytes_per_pixel, colour, bit_depth) in cases {
            let png = png_base64(2, 2, colour_type, depth, &vec![7; 4 * bytes_per_pixel]);
            let bitmap = bitmap(2, 2, png);
            assert_eq!(
                reason(bitmap.pixels()),
                BitmapError::NotGrayscale8 { colour, bit_depth },
                "{colour_type:?} at {depth:?}"
            );
            assert!(!bitmap.header().unwrap().is_grayscale_8());
        }
        // A palette, and 1-bit gray: headers alone, no image data.
        for (bit_depth, colour_type, colour) in [
            (8, 3, BitmapColour::Indexed),
            (1, 0, BitmapColour::Grayscale),
        ] {
            let bitmap = bitmap(2, 2, stored(&[ihdr(2, 2, bit_depth, colour_type)]));
            assert_eq!(
                reason(bitmap.pixels()),
                BitmapError::NotGrayscale8 { colour, bit_depth }
            );
        }
        let rgb = png_base64(1, 1, ColorType::Rgb, BitDepth::Eight, &[1, 2, 3]);
        assert_eq!(
            bitmap(1, 1, rgb).pixels().unwrap_err().to_string(),
            "bitmap `b1` cannot be read: its PNG is 8-bit RGB, not 8-bit grayscale"
        );
    }

    /// Security review M4: a picture with a side of 0, or one above
    /// `MAX_BITMAP_SIDE_PX`, is refused from the bitmap's numbers and the
    /// PNG's header. The PNGs here are a header and nothing else, so a
    /// refusal that names the size shows no image data was asked for.
    #[test]
    fn empty_or_oversized_bitmap_is_refused_from_the_header() {
        assert_eq!(MAX_BITMAP_SIDE_PX, 8192);

        // 0 × 0: the bitmap's own numbers refuse it.
        let empty = stored(&[ihdr(0, 0, 8, 0)]);
        for (width_px, height_px) in [(0, 0), (0, 4), (4, 0)] {
            assert_eq!(
                reason(bitmap(width_px, height_px, empty.clone()).pixels()),
                BitmapError::Empty {
                    width_px,
                    height_px
                }
            );
        }
        // A PNG that says 0 × 0 under numbers that do not: no PNG at all.
        assert!(matches!(
            reason(bitmap(4, 4, empty.clone()).pixels()),
            BitmapError::Png { .. }
        ));
        assert!(matches!(
            reason(bitmap(4, 4, empty).header()),
            BitmapError::Png { .. }
        ));

        // 9000 × 10: above the limit, whichever says so.
        let wide = stored(&[ihdr(9000, 10, 8, 0)]);
        let too_large = BitmapError::TooLarge {
            width_px: 9000,
            height_px: 10,
            limit_px: 8192,
        };
        assert_eq!(reason(bitmap(9000, 10, wide.clone()).pixels()), too_large);
        // The bitmap's numbers lie; the header is checked all the same.
        assert_eq!(reason(bitmap(10, 10, wide.clone()).pixels()), too_large);
        // The numbers alone, over a picture within the limit.
        assert_eq!(
            reason(bitmap(10, 8193, sample_bitmap().png_base64).pixels()),
            BitmapError::TooLarge {
                width_px: 10,
                height_px: 8193,
                limit_px: 8192,
            }
        );
        // Reading the header refuses nothing by size: validation does.
        let header = bitmap(9000, 10, wide.clone()).header().unwrap();
        assert_eq!((header.width_px, header.height_px), (9000, 10));
        assert_eq!(
            bitmap(9000, 10, wide).pixels().unwrap_err().to_string(),
            "bitmap `b1` cannot be read: it is 9000 × 10 px, and a side is at most 8192 px"
        );

        // The largest sides the format has overflow nothing.
        let huge = stored(&[ihdr(u32::MAX, u32::MAX, 8, 0)]);
        assert!(matches!(
            reason(bitmap(u32::MAX, u32::MAX, huge.clone()).pixels()),
            BitmapError::TooLarge { .. }
        ));
        assert!(matches!(
            reason(bitmap(4, 4, huge).pixels()),
            BitmapError::TooLarge { .. } | BitmapError::Png { .. }
        ));
    }

    /// Security review M4: a small file must not make the decoder allocate
    /// without bound. A 4 × 4 picture carries a colour-profile chunk whose
    /// zlib stream inflates to twice 8192 × 8192 bytes, from a string
    /// under 1 MB, and the same stream in a text chunk. `pixels` skips
    /// both chunks, so the picture reads as if they were not there, at no
    /// cost; and the limit it decodes under, `MAX_BITMAP_SIDE_PX² + one
    /// row` bytes, holds where a chunk is inflated: a decoder told to read
    /// the profile, under that limit, drops it rather than hold it.
    ///
    /// The design's name for this test ended `…_is_refused_by_the_decoder_
    /// limit`. `png` 0.18 never fails a picture for its profile (it drops
    /// one it cannot hold), so the bomb is bounded, not refused (#644).
    #[test]
    fn decompression_bomb_is_bounded_by_the_decoder_limit() {
        let small = decoded(&gray_png_base64(4, 4, &[9; 16]));
        let idat = chunk(b"IDAT", &image_data(&small));
        let iend = chunk(b"IEND", &[]);
        // 16 400 rows of 8193 bytes: 134 MB, twice the limit.
        let stream = zeros_stream(16_400);
        let mut payload = b"bomb\0\0".to_vec();
        payload.extend_from_slice(&stream);

        assert_eq!(decoder_limits().bytes, 8192 * 8192 + 8192);
        for kind in [b"iCCP", b"zTXt"] {
            let png = stored(&[
                ihdr(4, 4, 8, 0),
                chunk(kind, &payload),
                idat.clone(),
                iend.clone(),
            ]);
            assert!(png.len() < 1_000_000, "{} characters", png.len());
            let bomb = bitmap(4, 4, png);
            assert_eq!(bomb.check_stored(), Ok(()));
            assert_eq!(bomb.header().unwrap().width_px, 4);
            assert_eq!(bomb.pixels().unwrap().bytes, vec![9; 16]);
        }

        // The limit itself, on a decoder that does read the profile: a
        // profile within it is held, the bomb is not.
        let profile_of = |payload: &[u8]| {
            let png = decoded(&stored(&[
                ihdr(4, 4, 8, 0),
                chunk(b"iCCP", payload),
                idat.clone(),
                iend.clone(),
            ]));
            let decoder = png::Decoder::new_with_limits(Cursor::new(png), decoder_limits());
            let reader = decoder.read_info().unwrap();
            reader.info().icc_profile.as_ref().map(|p| p.len())
        };
        let mut modest = b"fine\0\0".to_vec();
        modest.extend_from_slice(&zeros_stream(2));
        assert_eq!(profile_of(&modest), Some(2 * 8193));
        assert_eq!(profile_of(&payload), None);
    }

    /// Security review M4: a header that says more than the image data
    /// holds. Under numbers that disagree it is refused before decoding;
    /// under numbers that agree the decoder runs out of data and says so.
    /// Even at the largest size a picture may claim, the cost is that
    /// picture's buffer and no more.
    #[test]
    fn header_lying_about_size_is_refused() {
        let small = decoded(&gray_png_base64(4, 4, &[9; 16]));
        let idat = chunk(b"IDAT", &image_data(&small));
        let iend = chunk(b"IEND", &[]);
        let lying = |side: u32| stored(&[ihdr(side, side, 8, 0), idat.clone(), iend.clone()]);

        assert_eq!(
            reason(bitmap(4, 4, lying(64)).pixels()),
            BitmapError::SizeMismatch {
                width_px: 4,
                height_px: 4,
                png_width_px: 64,
                png_height_px: 64,
            }
        );
        for side in [64, MAX_BITMAP_SIDE_PX] {
            let bitmap = bitmap(side, side, lying(side));
            assert_eq!(bitmap.check_stored(), Ok(()), "{side}: the header reads");
            assert!(
                matches!(reason(bitmap.pixels()), BitmapError::Png { .. }),
                "{side}"
            );
        }
        // A header that says less than the data holds reads its own size.
        let big = decoded(&gray_png_base64(8, 8, &[3; 64]));
        let short = stored(&[
            ihdr(8, 2, 8, 0),
            chunk(b"IDAT", &image_data(&big)),
            iend.clone(),
        ]);
        assert_eq!(bitmap(8, 2, short).pixels().unwrap().bytes, vec![3; 16]);
    }

    /// A `.dcv` file is untrusted: text that is not base64, base64 that is
    /// not a PNG, and a PNG cut short are each a typed error, from `load`'s
    /// check where the header shows it and from `pixels` always.
    #[test]
    fn undecodable_bitmap_is_a_typed_error() {
        let good = sample_bitmap().png_base64;

        // Truncated base64: the last character cut off, or one too many.
        let cut = good[..good.len() - 1].to_owned();
        for bitmap in [bitmap(4, 2, cut), bitmap(4, 2, format!("{good}="))] {
            assert!(matches!(
                reason(bitmap.check_stored()),
                BitmapError::Base64 { .. }
            ));
            assert!(matches!(
                reason(bitmap.pixels()),
                BitmapError::Base64 { .. }
            ));
        }
        // Whole groups cut off: base64 still, but the PNG ends early.
        let short = good[..good.len() - 16].to_owned();
        let bitmap_short = bitmap(4, 2, short);
        assert_eq!(bitmap_short.check_stored(), Ok(()));
        assert!(matches!(
            reason(bitmap_short.pixels()),
            BitmapError::Png { .. }
        ));

        // Not the standard alphabet, a line break, nothing at all.
        for text in [
            good.replacen('A', "-", 1),
            format!("{}\n{}", &good[..40], &good[40..]),
            format!(" {good}"),
        ] {
            let bitmap = bitmap(4, 2, text.clone());
            assert!(
                matches!(reason(bitmap.check_stored()), BitmapError::Base64 { .. }),
                "{text}"
            );
            assert!(
                matches!(reason(bitmap.pixels()), BitmapError::Base64 { .. }),
                "{text}"
            );
        }
        // Base64, but no PNG: empty, a word, a header cut inside `IHDR`.
        for text in [
            String::new(),
            STANDARD.encode("not a picture at all, only some text"),
            good[..20].to_owned(),
        ] {
            let bitmap = bitmap(4, 2, text.clone());
            assert!(
                matches!(reason(bitmap.header()), BitmapError::Png { .. }),
                "{text}"
            );
            assert!(
                matches!(reason(bitmap.check_stored()), BitmapError::Png { .. }),
                "{text}"
            );
            assert!(
                matches!(reason(bitmap.pixels()), BitmapError::Png { .. }),
                "{text}"
            );
        }
        // A bad CRC on the header, and image data that is not zlib.
        let mut png = decoded(&good);
        png[20] ^= 0xff;
        assert!(matches!(
            reason(bitmap(4, 2, STANDARD.encode(&png)).header()),
            BitmapError::Png { .. }
        ));
        let garbage = stored(&[
            ihdr(4, 2, 8, 0),
            chunk(b"IDAT", &[0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4]),
            chunk(b"IEND", &[]),
        ]);
        let bitmap_garbage = bitmap(4, 2, garbage);
        assert_eq!(bitmap_garbage.check_stored(), Ok(()));
        assert!(matches!(
            reason(bitmap_garbage.pixels()),
            BitmapError::Png { .. }
        ));
    }

    /// `load`'s base64 check reads the text in pieces without holding the
    /// decoded bytes, and agrees with decoding it whole at every length
    /// round a piece's end, padded or not, and on padding or a stray
    /// character anywhere.
    #[test]
    fn base64_check_agrees_with_decoding_the_whole_text() {
        let agree = |text: &str| {
            assert_eq!(
                check_base64(text).is_ok(),
                STANDARD.decode(text).is_ok(),
                "{} characters: {:?}",
                text.len(),
                check_base64(text)
            );
        };
        for bytes in [
            0, 1, 2, 3, 4, 3070, 3071, 3072, 3073, 3074, 6143, 6144, 6145, 9217,
        ] {
            let data: Vec<u8> = (0..bytes).map(|i| (i * 31 % 251) as u8).collect();
            let text = STANDARD.encode(&data);
            assert_eq!(check_base64(&text), Ok(()), "{bytes} bytes");
            agree(&text);
            for at in [
                0,
                1,
                4095,
                4096,
                4097,
                8191,
                8192,
                text.len().saturating_sub(1),
            ] {
                if at >= text.len() {
                    continue;
                }
                for stray in ["=", "*", "\n"] {
                    let mut bad = text.clone();
                    bad.replace_range(at..=at, stray);
                    agree(&bad);
                }
                agree(&text[..at]);
            }
        }
        // Padding that ends a piece, with more after it.
        let padded = format!("{}QQ=={}", "QUJD".repeat(1023), "QUJD".repeat(4));
        assert!(check_base64(&padded).is_err());
        agree(&padded);
    }
}
