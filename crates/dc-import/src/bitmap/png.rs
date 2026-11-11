// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! PNG decoding and the stored PNG's encoding, with the `png` crate.

use std::io::Cursor;

use dc_project::Grayscale;
use png::{BitDepth, ColorType, Decoder, Encoder, InterlaceInfo, Limits, Transformations};

use super::gray::Converter;
use super::{BITMAP_PNG_COMPRESSION, Decoded, PNG_DECODER_LIMIT_BYTES, check_size, gray_buffer};
use crate::ImportError;

/// Decodes a PNG: the header first, its size checked against the side
/// limit before any image data is read; then a row at a time, palette and
/// low bit depths expanded to 8 bits and `tRNS` to alpha by the decoder,
/// 16-bit samples taken by their high byte here, each row converted to
/// gray into the picture.
pub(super) fn decode(bytes: &[u8]) -> Result<Decoded, ImportError> {
    let mut decoder = Decoder::new_with_limits(
        Cursor::new(bytes),
        Limits {
            bytes: PNG_DECODER_LIMIT_BYTES,
        },
    );
    // The chunks a PNG compresses beside its image data: nothing of them
    // is needed, so a small one that inflates to gigabytes costs nothing.
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let header = decoder.read_header_info().map_err(decode_error)?;
    let (width_px, height_px) = (header.width, header.height);
    let sixteen_bit = header.bit_depth == BitDepth::Sixteen;
    check_size(width_px, height_px)?;

    decoder.set_transformations(Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(decode_error)?;
    let (colour, depth) = reader.output_color_type();
    let channels = match colour {
        ColorType::Grayscale => 1,
        ColorType::GrayscaleAlpha => 2,
        ColorType::Rgb => 3,
        ColorType::Rgba => 4,
        // `EXPAND` turns a palette into RGB or RGBA.
        ColorType::Indexed => {
            return Err(ImportError::ImageDecode {
                reason: "the decoder left its palette unexpanded".to_owned(),
            });
        }
    };
    let bytes_per_sample = if depth == BitDepth::Sixteen { 2 } else { 1 };
    let width = usize::try_from(width_px).unwrap_or(usize::MAX);
    let mut pixels = gray_buffer(width_px, height_px)?;
    let mut converter = Converter::default();
    // One row of 8-bit samples, and one row of gray (for an interlaced
    // pass, which is placed into the picture afterwards).
    let mut samples = vec![0_u8; width.saturating_mul(channels)];
    let mut gray_row = vec![0_u8; width];
    let mut next_row = 0_usize;
    while let Some(row) = reader.next_interlaced_row().map_err(decode_error)? {
        let data = row.data();
        let count = data.len() / (channels * bytes_per_sample);
        if count > width {
            return Err(row_error());
        }
        let eight_bit: &[u8] = if bytes_per_sample == 2 {
            // Big-endian: the high byte comes first.
            let (pairs, _) = data.as_chunks::<2>();
            for (out, [high, _]) in samples.iter_mut().zip(pairs) {
                *out = *high;
            }
            samples.get(..count * channels).ok_or_else(row_error)?
        } else {
            data
        };
        match row.interlace() {
            InterlaceInfo::Null(_) => {
                let start = next_row.checked_mul(width).ok_or_else(row_error)?;
                let out = pixels
                    .get_mut(start..start + count)
                    .ok_or_else(row_error)?;
                converter.row(eight_bit, channels, out);
                next_row += 1;
            }
            InterlaceInfo::Adam7(info) => {
                let gray = gray_row.get_mut(..count).ok_or_else(row_error)?;
                converter.row(eight_bit, channels, gray);
                png::expand_interlaced_row(&mut pixels, width, gray, info, 8);
            }
        }
    }
    reader.finish().map_err(decode_error)?;
    Ok(Decoded {
        pixels: Grayscale {
            width_px,
            height_px,
            bytes: pixels,
        },
        sixteen_bit,
        seen: converter,
    })
}

/// A row the decoder handed back that does not fit the picture.
fn row_error() -> ImportError {
    ImportError::ImageDecode {
        reason: "a row of image data does not fit the picture's size".to_owned(),
    }
}

/// A decoder error as an [`ImportError`].
fn decode_error(err: png::DecodingError) -> ImportError {
    ImportError::ImageDecode {
        reason: format!("PNG: {err}"),
    }
}

/// Encodes the PNG a bitmap stores ([`super::encode`]).
pub(super) fn encode(pixels: &Grayscale) -> Result<Vec<u8>, ImportError> {
    let (width_px, height_px) = (pixels.width_px, pixels.height_px);
    check_size(width_px, height_px)?;
    let count = usize::try_from(u64::from(width_px) * u64::from(height_px)).ok();
    if count != Some(pixels.bytes.len()) {
        return Err(ImportError::ImageDecode {
            reason: format!(
                "{} samples do not fill a {width_px} × {height_px} px picture",
                pixels.bytes.len()
            ),
        });
    }
    let encode_error = |err: png::EncodingError| ImportError::ImageDecode {
        reason: format!("PNG encoder: {err}"),
    };
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, width_px, height_px);
    encoder.set_color(ColorType::Grayscale);
    encoder.set_depth(BitDepth::Eight);
    // The level first: `set_compression` also picks a filter, which the
    // next line replaces with the one the design names.
    encoder.set_compression(BITMAP_PNG_COMPRESSION);
    encoder.set_filter(png::Filter::Sub);
    let mut writer = encoder.write_header().map_err(encode_error)?;
    writer
        .write_image_data(&pixels.bytes)
        .map_err(encode_error)?;
    writer.finish().map_err(encode_error)?;
    Ok(out)
}
