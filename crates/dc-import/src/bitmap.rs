// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pictures: a PNG or JPEG decoded to the 8-bit grayscale a project's
//! bitmap stores, and that grayscale encoded as the PNG it stores
//! (`docs/specs/phase-3/design.md`, "dc-import" and "Bitmap import and
//! storage", steps 1 to 3; security review M4).
//!
//! A picture is untrusted input. Before any image data is decoded, the
//! picture's header is read and a side above
//! [`dc_project::MAX_BITMAP_SIDE_PX`] is refused
//! ([`ImportError::ImageTooLarge`]); the decoders then run under limits of
//! their own set to the same bound, so a header that lies cannot make
//! them allocate past it:
//!
//! - **PNG** (`png`): every colour type and bit depth, palette and low bit
//!   depths expanded, 16-bit samples taken by their high byte. Text and
//!   colour-profile chunks are skipped, the decoder holds at most
//!   [`PNG_DECODER_LIMIT_BYTES`] of its own, and the picture is converted
//!   a row at a time, so the memory is the gray picture and a row.
//! - **JPEG** (`zune-jpeg`): baseline and progressive, CMYK converted to
//!   RGB by the decoder, in strict mode, so a truncated or corrupt stream
//!   is an error and not a picture padded with gray; its own width and
//!   height limits are the side limit, and its scans at most
//!   [`JPEG_MAX_SCANS`].
//!
//! Every pixel is then converted to gray by one formula, Rec. 601
//! luminance after compositing on white (`gray.rs`), so the two formats
//! read alike. Pure: bytes in, values out.

mod gray;
mod jpeg;
mod png;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use dc_project::{Grayscale, MAX_BITMAP_SIDE_PX};

use crate::{ImportError, ImportWarning};

/// The deflate level of the PNG a bitmap stores: `png`'s default level,
/// pinned by name, so the bytes are a function of the pixels within one
/// release of the encoder.
pub const BITMAP_PNG_COMPRESSION: ::png::Compression = ::png::Compression::Balanced;

/// The most the PNG decoder may hold of its own while it reads a picture,
/// beside the gray picture: `MAX_BITMAP_SIDE_PX²` bytes. Its row buffers
/// need a row or two (at most 64 KiB at 8192 pixels of 16-bit RGBA); the
/// rest bounds what it may buffer of a chunk.
pub const PNG_DECODER_LIMIT_BYTES: usize = (MAX_BITMAP_SIDE_PX as usize).pow(2);

/// The most scans a progressive JPEG may have: `zune-jpeg`'s default,
/// pinned by name. A crafted file of thousands of scans would otherwise
/// cost a pass over the picture each.
pub const JPEG_MAX_SCANS: usize = 100;

/// A picture format DreamCarve reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    /// Portable Network Graphics, `.png`.
    Png,
    /// JPEG, `.jpg` or `.jpeg`.
    Jpeg,
}

impl ImageFormat {
    /// The format a file extension names (`png`, `jpg`, `jpeg`, any case),
    /// or `None` for any other.
    #[must_use]
    pub fn from_extension(extension: &str) -> Option<Self> {
        if extension.eq_ignore_ascii_case("png") {
            Some(Self::Png)
        } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
            Some(Self::Jpeg)
        } else {
            None
        }
    }

    /// The format whose signature `bytes` start with, or `None`.
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&PNG_SIGNATURE) {
            Some(Self::Png)
        } else if bytes.starts_with(&JPEG_SIGNATURE) {
            Some(Self::Jpeg)
        } else {
            None
        }
    }

    /// The format's name, as messages give it: `PNG` or `JPEG`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
        }
    }
}

/// The eight bytes every PNG starts with.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
/// A JPEG's start-of-image marker and the first byte of the next marker.
const JPEG_SIGNATURE: [u8; 3] = [0xff, 0xd8, 0xff];

/// What a decoder produced: the gray picture and what it changed on the
/// way.
struct Decoded {
    pixels: Grayscale,
    /// The samples were 16-bit.
    sixteen_bit: bool,
    /// What the gray conversion found.
    seen: gray::Converter,
}

/// Decodes `bytes`, a picture in `format` (from the file's extension), into
/// 8-bit grayscale, row-major from the top row, with the warnings of what
/// was converted, in this order: [`ImportWarning::Image16Bit`],
/// [`ImportWarning::ImageAlphaFlattened`],
/// [`ImportWarning::ImageColourConverted`].
///
/// # Errors
///
/// [`ImportError::ImageFormatMismatch`] when the bytes are the other
/// format's; [`ImportError::ImageTooLarge`] when the header says a side
/// is above [`MAX_BITMAP_SIDE_PX`], before any image data is decoded;
/// [`ImportError::ImageDecode`] for anything else that cannot be read
/// (not a picture, truncated, corrupt, no pixels, a decoder limit, no
/// memory for the picture).
pub fn import(
    bytes: &[u8],
    format: ImageFormat,
) -> Result<(Grayscale, Vec<ImportWarning>), ImportError> {
    match ImageFormat::sniff(bytes) {
        Some(found) if found != format => {
            return Err(ImportError::ImageFormatMismatch {
                extension: format,
                found,
            });
        }
        Some(_) => {}
        None => {
            return Err(ImportError::ImageDecode {
                reason: format!("it does not start with a {} signature", format.name()),
            });
        }
    }
    let decoded = match format {
        ImageFormat::Png => png::decode(bytes)?,
        ImageFormat::Jpeg => jpeg::decode(bytes)?,
    };
    let mut warnings = Vec::new();
    if decoded.sixteen_bit {
        warnings.push(ImportWarning::Image16Bit);
    }
    if decoded.seen.alpha {
        warnings.push(ImportWarning::ImageAlphaFlattened);
    }
    if decoded.seen.colour {
        warnings.push(ImportWarning::ImageColourConverted);
    }
    Ok((decoded.pixels, warnings))
}

/// The PNG a bitmap stores for `pixels`: 8-bit grayscale, no interlace,
/// every row filtered `Sub`, deflated at [`BITMAP_PNG_COMPRESSION`], no
/// ancillary chunk, so the bytes are a function of the pixels.
///
/// # Errors
///
/// [`ImportError::ImageDecode`] when `pixels` does not hold `width_px ×
/// height_px` samples, a side is 0 or above [`MAX_BITMAP_SIDE_PX`], or the
/// encoder fails.
pub fn encode(pixels: &Grayscale) -> Result<Vec<u8>, ImportError> {
    png::encode(pixels)
}

/// [`encode`] as the text a bitmap's `png_base64` holds: base64, standard
/// alphabet, padded, one line.
///
/// # Errors
///
/// As [`encode`].
pub fn stored(pixels: &Grayscale) -> Result<String, ImportError> {
    Ok(STANDARD.encode(encode(pixels)?))
}

/// Refuses a picture with a side above [`MAX_BITMAP_SIDE_PX`] or of 0,
/// from its header's numbers.
fn check_size(width_px: u32, height_px: u32) -> Result<(), ImportError> {
    if width_px > MAX_BITMAP_SIDE_PX || height_px > MAX_BITMAP_SIDE_PX {
        return Err(ImportError::ImageTooLarge {
            width_px,
            height_px,
            limit_px: MAX_BITMAP_SIDE_PX,
        });
    }
    if width_px == 0 || height_px == 0 {
        return Err(ImportError::ImageDecode {
            reason: format!("it is {width_px} × {height_px} px, and has no pixels"),
        });
    }
    Ok(())
}

/// A zeroed buffer of `width_px × height_px` gray samples, reserved first
/// so a machine that cannot spare them is told so and the process is not
/// aborted. The sides are already checked, so the count fits.
fn gray_buffer(width_px: u32, height_px: u32) -> Result<Vec<u8>, ImportError> {
    let count = usize::try_from(width_px)
        .ok()
        .zip(usize::try_from(height_px).ok())
        .and_then(|(w, h)| w.checked_mul(h))
        .ok_or(ImportError::ImageTooLarge {
            width_px,
            height_px,
            limit_px: MAX_BITMAP_SIDE_PX,
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(count)
        .map_err(|_| ImportError::ImageDecode {
            reason: format!("there is no memory for its {width_px} × {height_px} pixels"),
        })?;
    bytes.resize(count, 0);
    Ok(bytes)
}

#[cfg(test)]
#[allow(
    clippy::cast_possible_truncation,
    reason = "test samples are small numbers wrapped into a byte on purpose"
)]
mod tests {
    use super::*;
    use dc_project::{Bitmap, BitmapId};

    /// The committed fixtures (`tools/parity/photo-fixtures.ts`).
    const COLOUR_JPG: &[u8] = include_bytes!("../../../tests/fixtures/photo/colour.jpg");
    const COLOUR_PROGRESSIVE_JPG: &[u8] =
        include_bytes!("../../../tests/fixtures/photo/colour-progressive.jpg");
    const GRADIENT_PNG: &[u8] = include_bytes!("../../../tests/fixtures/photo/gradient.png");
    /// Rec. 601 luminance of the JPEGs' four blocks, rgb(200, 60, 40),
    /// rgb(40, 160, 80), rgb(50, 70, 200) and rgb(128, 128, 128).
    const COLOUR_BLOCK_GRAYS: [u8; 4] = [100, 115, 79, 128];

    /// A PNG of `samples`, as an encoder of any program might write it.
    fn png_of(
        width_px: u32,
        height_px: u32,
        colour: ::png::ColorType,
        depth: ::png::BitDepth,
        samples: &[u8],
        palette: Option<(&[u8], Option<&[u8]>)>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = ::png::Encoder::new(&mut out, width_px, height_px);
        encoder.set_color(colour);
        encoder.set_depth(depth);
        if let Some((rgb, trns)) = palette {
            encoder.set_palette(rgb.to_vec());
            if let Some(trns) = trns {
                encoder.set_trns(trns.to_vec());
            }
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(samples).unwrap();
        writer.finish().unwrap();
        out
    }

    /// An 8-bit gray PNG of `samples`.
    fn gray_png(width_px: u32, height_px: u32, samples: &[u8]) -> Vec<u8> {
        png_of(
            width_px,
            height_px,
            ::png::ColorType::Grayscale,
            ::png::BitDepth::Eight,
            samples,
            None,
        )
    }

    /// The CRC-32 a PNG chunk ends with (ISO 3309), bit by bit.
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

    /// A PNG of a signature, an 8-bit gray `IHDR` of the given size, and
    /// `rest`.
    fn png_with_header(width_px: u32, height_px: u32, rest: &[Vec<u8>]) -> Vec<u8> {
        let mut data = width_px.to_be_bytes().to_vec();
        data.extend_from_slice(&height_px.to_be_bytes());
        data.extend_from_slice(&[8, 0, 0, 0, 0]);
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend(chunk(b"IHDR", &data));
        for c in rest {
            png.extend_from_slice(c);
        }
        png
    }

    /// The `IDAT` chunks of `png`, as chunks.
    fn idat_of(png: &[u8]) -> Vec<Vec<u8>> {
        let mut rest = &png[8..];
        let mut out = Vec::new();
        while rest.len() >= 12 {
            let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
            if &rest[4..8] == b"IDAT" {
                out.push(rest[..12 + len].to_vec());
            }
            rest = &rest[12 + len..];
        }
        out
    }

    /// A JPEG of a start of image, a baseline (`0xc0`) or progressive
    /// (`0xc2`) frame header of the given size with three components, and
    /// an end of image: a header and no image data.
    fn jpeg_header_only(sof: u8, width_px: u16, height_px: u16) -> Vec<u8> {
        let mut jpeg = vec![0xff, 0xd8, 0xff, sof, 0, 17, 8];
        jpeg.extend_from_slice(&height_px.to_be_bytes());
        jpeg.extend_from_slice(&width_px.to_be_bytes());
        jpeg.extend_from_slice(&[3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0]);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    /// `jpeg` with its frame header's size replaced.
    fn jpeg_resized(jpeg: &[u8], width_px: u16, height_px: u16) -> Vec<u8> {
        let mut out = jpeg.to_vec();
        let sof = out
            .windows(2)
            .position(|w| w[0] == 0xff && (w[1] == 0xc0 || w[1] == 0xc2))
            .unwrap();
        out[sof + 5..sof + 7].copy_from_slice(&height_px.to_be_bytes());
        out[sof + 7..sof + 9].copy_from_slice(&width_px.to_be_bytes());
        out
    }

    fn decode_error<T>(result: Result<T, ImportError>) -> String {
        match result {
            Err(ImportError::ImageDecode { reason }) => reason,
            Err(other) => panic!("expected ImageDecode, got {other:?}"),
            Ok(_) => panic!("expected ImageDecode, got a picture"),
        }
    }

    /// The mean of the 8 × 8 block at (`column`, `row`) of `pixels`.
    fn block_mean(pixels: &Grayscale, column: u32, row: u32) -> f64 {
        let mut sum = 0_u32;
        for j in row..row + 8 {
            for i in column..column + 8 {
                sum += u32::from(pixels.bytes[(j * pixels.width_px + i) as usize]);
            }
        }
        f64::from(sum) / 64.0
    }

    #[test]
    fn png_gray_8_reads_as_is() {
        let samples = [0, 1, 127, 128, 254, 255];
        let (pixels, warnings) = import(&gray_png(3, 2, &samples), ImageFormat::Png).unwrap();
        assert_eq!(
            pixels,
            Grayscale {
                width_px: 3,
                height_px: 2,
                bytes: samples.to_vec(),
            }
        );
        assert_eq!(warnings, []);

        // The committed fixture: 64 × 32, black at the left edge, white
        // at the right, the white block at column 8, the black at 48,
        // both from row 12.
        let (gradient, warnings) = import(GRADIENT_PNG, ImageFormat::Png).unwrap();
        assert_eq!((gradient.width_px, gradient.height_px), (64, 32));
        assert_eq!(warnings, []);
        let at = |i: u32, j: u32| gradient.bytes[(j * 64 + i) as usize];
        assert_eq!((at(0, 0), at(63, 0), at(31, 0), at(32, 0)), (0, 255, 125, 130));
        assert!((block_mean(&gradient, 8, 12) - 255.0).abs() < f64::EPSILON);
        assert!(block_mean(&gradient, 48, 12).abs() < f64::EPSILON);
        assert_eq!((at(7, 12), at(16, 12), at(8, 11), at(8, 20)), (28, 65, 32, 32));
    }

    /// Rec. 601: `round(0.299 R + 0.587 G + 0.114 B)`.
    #[test]
    fn png_rgb_converts_by_luminance_and_warns() {
        let samples = [
            255, 0, 0, // 76.245
            0, 255, 0, // 149.685
            0, 0, 255, // 29.07
            10, 20, 30, // 18.15
            77, 77, 77, // gray
            200, 60, 40, // 99.98
        ];
        let png = png_of(
            3,
            2,
            ::png::ColorType::Rgb,
            ::png::BitDepth::Eight,
            &samples,
            None,
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [76, 150, 29, 18, 77, 100]);
        assert_eq!(warnings, [ImportWarning::ImageColourConverted]);

        // An RGB picture of grays only had no colour to convert.
        let grays = png_of(
            2,
            1,
            ::png::ColorType::Rgb,
            ::png::BitDepth::Eight,
            &[9, 9, 9, 200, 200, 200],
            None,
        );
        let (pixels, warnings) = import(&grays, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [9, 200]);
        assert_eq!(warnings, []);
    }

    /// Alpha is composited on white first: `c' = (a·c + (255 − a)·255) /
    /// 255`, then luminance.
    #[test]
    fn png_rgba_flattens_on_white() {
        let samples = [
            0, 0, 0, 0, // transparent: white
            0, 0, 0, 255, // opaque black
            0, 0, 0, 128, // half: 127/255 of white
            255, 0, 0, 255, // opaque red
        ];
        let png = png_of(
            2,
            2,
            ::png::ColorType::Rgba,
            ::png::BitDepth::Eight,
            &samples,
            None,
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [255, 0, 127, 76]);
        assert_eq!(
            warnings,
            [
                ImportWarning::ImageAlphaFlattened,
                ImportWarning::ImageColourConverted
            ]
        );

        // Gray with alpha: flattened, no colour.
        let png = png_of(
            2,
            1,
            ::png::ColorType::GrayscaleAlpha,
            ::png::BitDepth::Eight,
            &[0, 0, 100, 255],
            None,
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [255, 100]);
        assert_eq!(warnings, [ImportWarning::ImageAlphaFlattened]);

        // An alpha channel that is opaque everywhere changes nothing.
        let png = png_of(
            1,
            1,
            ::png::ColorType::Rgba,
            ::png::BitDepth::Eight,
            &[50, 50, 50, 255],
            None,
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [50]);
        assert_eq!(warnings, []);
    }

    #[test]
    fn png_16_bit_takes_the_high_byte() {
        // Big-endian samples 0x1234, 0xff00, 0x00ff, 0xffff.
        let samples = [0x12, 0x34, 0xff, 0x00, 0x00, 0xff, 0xff, 0xff];
        let png = png_of(
            2,
            2,
            ::png::ColorType::Grayscale,
            ::png::BitDepth::Sixteen,
            &samples,
            None,
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [0x12, 0xff, 0x00, 0xff]);
        assert_eq!(warnings, [ImportWarning::Image16Bit]);

        // 16-bit RGB: the high bytes, then luminance.
        let png = png_of(
            1,
            1,
            ::png::ColorType::Rgb,
            ::png::BitDepth::Sixteen,
            &[0xff, 0x01, 0x00, 0xfe, 0x00, 0x02],
            None,
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [76]);
        assert_eq!(
            warnings,
            [ImportWarning::Image16Bit, ImportWarning::ImageColourConverted]
        );
    }

    #[test]
    fn png_palette_expands() {
        // 2-bit indices into red, black, white; four pixels a row.
        let palette = [255, 0, 0, 0, 0, 0, 255, 255, 255];
        let png = png_of(
            4,
            1,
            ::png::ColorType::Indexed,
            ::png::BitDepth::Two,
            &[0b0001_1001],
            Some((&palette, None)),
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [76, 0, 255, 0]);
        assert_eq!(warnings, [ImportWarning::ImageColourConverted]);

        // A transparent palette entry (`tRNS`) flattens to white.
        let png = png_of(
            2,
            1,
            ::png::ColorType::Indexed,
            ::png::BitDepth::Eight,
            &[0, 1],
            Some((&palette, Some(&[0]))),
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [255, 0]);
        assert_eq!(warnings, [ImportWarning::ImageAlphaFlattened]);

        // 1-bit gray expands to 0 and 255.
        let png = png_of(
            3,
            1,
            ::png::ColorType::Grayscale,
            ::png::BitDepth::One,
            &[0b1010_0000],
            None,
        );
        let (pixels, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(pixels.bytes, [255, 0, 255]);
        assert_eq!(warnings, []);
    }

    /// An interlaced PNG reads as the same picture.
    #[test]
    fn png_interlaced_reads_as_the_same_picture() {
        let samples: Vec<u8> = (0..13 * 9).map(|k| (k * 7 % 256) as u8).collect();
        let mut out = Vec::new();
        let mut encoder = ::png::Encoder::new(&mut out, 13, 9);
        encoder.set_color(::png::ColorType::Grayscale);
        encoder.set_depth(::png::BitDepth::Eight);
        let mut info = encoder.write_header().unwrap();
        info.write_image_data(&samples).unwrap();
        info.finish().unwrap();
        let plain = import(&out, ImageFormat::Png).unwrap().0;
        assert_eq!(plain.bytes, samples);

        // The same samples, Adam7-interlaced by hand: the passes' pixels
        // in pass order, each pass line with a filter byte of 0.
        let passes: [(u32, u32, u32, u32); 7] = [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ];
        let mut raw = Vec::new();
        for (x0, y0, dx, dy) in passes {
            if x0 >= 13 || y0 >= 9 {
                continue;
            }
            for y in (y0..9).step_by(dy as usize) {
                raw.push(0);
                for x in (x0..13).step_by(dx as usize) {
                    raw.push(samples[(y * 13 + x) as usize]);
                }
            }
        }
        let zlib = zlib_stored(&raw);
        let mut ihdr = 13_u32.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&9_u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 0, 0, 0, 1]);
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend(chunk(b"IHDR", &ihdr));
        png.extend(chunk(b"IDAT", &zlib));
        png.extend(chunk(b"IEND", &[]));
        let (interlaced, warnings) = import(&png, ImageFormat::Png).unwrap();
        assert_eq!(interlaced, plain);
        assert_eq!(warnings, []);
    }

    /// `data` as a zlib stream of stored (uncompressed) deflate blocks.
    fn zlib_stored(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x78, 0x01];
        let blocks: Vec<&[u8]> = data.chunks(65_535).collect();
        for (k, block) in blocks.iter().enumerate() {
            out.push(u8::from(k + 1 == blocks.len()));
            let len = u16::try_from(block.len()).unwrap();
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(block);
        }
        let (mut a, mut b) = (1_u32, 0_u32);
        for byte in data {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        out.extend_from_slice(&((b << 16) | a).to_be_bytes());
        out
    }

    /// The colour JPEG fixture, baseline and progressive: four flat blocks
    /// whose gray is their colour's luminance within a level or two of
    /// the JPEG round trip.
    #[test]
    fn jpeg_reads_and_converts() {
        for (jpeg, what) in [
            (COLOUR_JPG, "baseline"),
            (COLOUR_PROGRESSIVE_JPG, "progressive"),
        ] {
            let (pixels, warnings) = import(jpeg, ImageFormat::Jpeg).unwrap();
            assert_eq!((pixels.width_px, pixels.height_px), (16, 16), "{what}");
            assert_eq!(pixels.bytes.len(), 256, "{what}");
            assert_eq!(warnings, [ImportWarning::ImageColourConverted], "{what}");
            for (k, expected) in COLOUR_BLOCK_GRAYS.iter().enumerate() {
                let (column, row) = (8 * (k as u32 % 2), 8 * (k as u32 / 2));
                let mean = block_mean(&pixels, column, row);
                assert!(
                    (mean - f64::from(*expected)).abs() <= 3.0,
                    "{what}: block {k} is {mean}, not {expected}"
                );
            }
        }
    }

    #[test]
    fn extension_and_magic_must_agree() {
        let png = gray_png(1, 1, &[7]);
        assert!(matches!(
            import(&png, ImageFormat::Jpeg),
            Err(ImportError::ImageFormatMismatch {
                extension: ImageFormat::Jpeg,
                found: ImageFormat::Png,
            })
        ));
        assert!(matches!(
            import(COLOUR_JPG, ImageFormat::Png),
            Err(ImportError::ImageFormatMismatch {
                extension: ImageFormat::Png,
                found: ImageFormat::Jpeg,
            })
        ));
        let message = import(&png, ImageFormat::Jpeg).unwrap_err().to_string();
        assert_eq!(
            message,
            "the file is named as a JPEG picture but holds a PNG; rename it to match"
        );
        // Neither signature: not a picture.
        for format in [ImageFormat::Png, ImageFormat::Jpeg] {
            let reason = decode_error(import(b"<svg xmlns=\"\"/>", format));
            assert!(reason.contains(format.name()), "{reason}");
            decode_error(import(&[], format));
        }
        // The extensions, any case.
        assert_eq!(ImageFormat::from_extension("PNG"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::from_extension("jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("JPeG"), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::from_extension("svg"), None);
        assert_eq!(ImageFormat::from_extension("gif"), None);
    }

    /// Security review M4: a side of 8193 is refused from the header. The
    /// pictures here are a header and no image data, so a refusal that
    /// names the size shows nothing was decoded; at 8192 the same file is
    /// refused for its missing data instead.
    #[test]
    fn too_large_is_refused() {
        assert_eq!(MAX_BITMAP_SIDE_PX, 8192);
        let too_large = |result: Result<(Grayscale, Vec<ImportWarning>), ImportError>| match result
        {
            Err(ImportError::ImageTooLarge {
                width_px,
                height_px,
                limit_px,
            }) => (width_px, height_px, limit_px),
            other => panic!("expected ImageTooLarge, got {other:?}"),
        };
        // PNG: wide, tall, and the largest the format has.
        assert_eq!(
            too_large(import(&png_with_header(8193, 1, &[]), ImageFormat::Png)),
            (8193, 1, 8192)
        );
        assert_eq!(
            too_large(import(&png_with_header(2, 8193, &[]), ImageFormat::Png)),
            (2, 8193, 8192)
        );
        let huge = png_with_header(u32::MAX >> 1, u32::MAX >> 1, &[]);
        assert_eq!(too_large(import(&huge, ImageFormat::Png)).2, 8192);
        decode_error(import(&png_with_header(8192, 8192, &[]), ImageFormat::Png));

        // JPEG, baseline and progressive.
        for sof in [0xc0, 0xc2] {
            assert_eq!(
                too_large(import(&jpeg_header_only(sof, 8193, 1), ImageFormat::Jpeg)),
                (8193, 1, 8192)
            );
            assert_eq!(
                too_large(import(&jpeg_header_only(sof, 4, 8193), ImageFormat::Jpeg)),
                (4, 8193, 8192)
            );
            assert_eq!(
                too_large(import(
                    &jpeg_header_only(sof, u16::MAX, u16::MAX),
                    ImageFormat::Jpeg
                )),
                (65_535, 65_535, 8192)
            );
            decode_error(import(&jpeg_header_only(sof, 8192, 8), ImageFormat::Jpeg));
        }
        // The real fixture with a frame header claiming more.
        assert_eq!(
            too_large(import(
                &jpeg_resized(COLOUR_JPG, 9000, 16),
                ImageFormat::Jpeg
            )),
            (9000, 16, 8192)
        );
        let message = import(&png_with_header(8193, 1, &[]), ImageFormat::Png)
            .unwrap_err()
            .to_string();
        assert_eq!(
            message,
            "the picture is 8193 × 1 px, and a side is at most 8192 px; scale it down first"
        );
    }

    /// Pictures are untrusted: truncated, corrupt, empty or lying ones are
    /// each a typed error, never a picture padded with gray or a panic.
    #[test]
    fn truncated_or_corrupt_picture_is_refused() {
        // PNG cut short anywhere after the signature.
        let png = gray_png(8, 8, &[3; 64]);
        for cut in [9, 20, 33, 40, png.len() - 13, png.len() - 1] {
            decode_error(import(&png[..cut], ImageFormat::Png));
        }
        // A bad CRC on the header, and image data that is not zlib.
        let mut bad_crc = png.clone();
        bad_crc[29] ^= 0xff;
        decode_error(import(&bad_crc, ImageFormat::Png));
        let garbage = png_with_header(
            4,
            2,
            &[
                chunk(b"IDAT", &[0xde, 0xad, 0xbe, 0xef, 1, 2, 3, 4]),
                chunk(b"IEND", &[]),
            ],
        );
        decode_error(import(&garbage, ImageFormat::Png));
        // A header of 0 × 0.
        decode_error(import(&png_with_header(0, 0, &[]), ImageFormat::Png));
        // A header that says more than the image data holds: 64 × 64 over
        // the data of 8 × 8.
        let mut lying = idat_of(&png);
        lying.push(chunk(b"IEND", &[]));
        decode_error(import(&png_with_header(64, 64, &lying), ImageFormat::Png));

        // JPEG cut short: in the headers, in the tables, and in the scan.
        // (A file that lacks only its end-of-image marker, its scan whole,
        // is every pixel of its picture and reads.)
        for cut in [3, 10, 30, 150, 170, COLOUR_JPG.len() - 20] {
            decode_error(import(&COLOUR_JPG[..cut], ImageFormat::Jpeg));
        }
        for cut in [150, COLOUR_PROGRESSIVE_JPG.len() - 12] {
            decode_error(import(&COLOUR_PROGRESSIVE_JPG[..cut], ImageFormat::Jpeg));
        }
        // A frame header claiming more than the scan holds, the file cut
        // short after it. (Where the scan instead ends at an end-of-image
        // marker, the decoder fills the rest, as JPEG decoders do; the
        // picture is no larger than its header, which the side limit
        // bounds.)
        for jpeg in [COLOUR_JPG, COLOUR_PROGRESSIVE_JPG] {
            let lying = jpeg_resized(jpeg, 64, 64);
            decode_error(import(&lying[..lying.len() - 2], ImageFormat::Jpeg));
        }
        // 0 × 0.
        decode_error(import(&jpeg_header_only(0xc0, 0, 0), ImageFormat::Jpeg));
        // A start of image and nothing else.
        decode_error(import(&[0xff, 0xd8, 0xff], ImageFormat::Jpeg));
    }

    #[test]
    fn encode_is_deterministic() {
        let pixels = Grayscale {
            width_px: 5,
            height_px: 3,
            bytes: (0..15).map(|k| (k * 17) as u8).collect(),
        };
        let first = encode(&pixels).unwrap();
        assert_eq!(encode(&pixels).unwrap(), first);
        assert_eq!(stored(&pixels).unwrap(), STANDARD.encode(&first));
        // 8-bit grayscale, not interlaced, and nothing but the header,
        // the image data and the end.
        let decoder = ::png::Decoder::new(std::io::Cursor::new(first.clone()));
        let reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!(info.color_type, ::png::ColorType::Grayscale);
        assert_eq!(info.bit_depth, ::png::BitDepth::Eight);
        assert!(!info.interlaced);
        let mut kinds = Vec::new();
        let mut rest = &first[8..];
        while rest.len() >= 12 {
            let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
            kinds.push(String::from_utf8(rest[4..8].to_vec()).unwrap());
            rest = &rest[12 + len..];
        }
        kinds.dedup();
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);

        // Pixels that do not fill their size are not encoded.
        let short = Grayscale {
            width_px: 5,
            height_px: 3,
            bytes: vec![0; 14],
        };
        decode_error(encode(&short));
        let empty = Grayscale {
            width_px: 0,
            height_px: 3,
            bytes: Vec::new(),
        };
        decode_error(encode(&empty));
    }

    /// `encode`, stored as a bitmap stores it, reads back through
    /// `Bitmap::pixels` as the same pixels; and imports as them too.
    #[test]
    fn encode_then_decode_round_trips() {
        let pixels = Grayscale {
            width_px: 7,
            height_px: 4,
            bytes: (0..28).map(|k| (255 - k * 9) as u8).collect(),
        };
        let bitmap = Bitmap {
            id: BitmapId::new("b1"),
            name: "ramp".into(),
            width_px: 7,
            height_px: 4,
            x_mm: 0.0,
            y_mm: 0.0,
            width_mm: 70.0,
            height_mm: 40.0,
            visible: true,
            locked: false,
            png_base64: stored(&pixels).unwrap(),
        };
        assert_eq!(bitmap.pixels().unwrap(), pixels);
        let png = encode(&pixels).unwrap();
        assert_eq!(import(&png, ImageFormat::Png).unwrap(), (pixels, Vec::new()));

        // A JPEG's gray, stored, reads back as that gray.
        let (gray, _) = import(COLOUR_JPG, ImageFormat::Jpeg).unwrap();
        let png = encode(&gray).unwrap();
        assert_eq!(import(&png, ImageFormat::Png).unwrap().0, gray);
    }
}
