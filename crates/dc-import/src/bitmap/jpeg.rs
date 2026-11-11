// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! JPEG decoding, with `zune-jpeg`.

use dc_project::{Grayscale, MAX_BITMAP_SIDE_PX};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use super::gray::Converter;
use super::{Decoded, JPEG_MAX_SCANS, check_size};
use crate::ImportError;

/// Decodes a JPEG: the frame header first, found by walking the markers
/// here, its size checked against the side limit before the decoder sees
/// the file; then `zune-jpeg`, in strict mode, under width and height
/// limits of the same bound and [`JPEG_MAX_SCANS`], to RGB (CMYK and YCCK
/// converted by it), converted to gray in place.
pub(super) fn decode(bytes: &[u8]) -> Result<Decoded, ImportError> {
    let (width_px, height_px) = frame_size(bytes)?;
    check_size(width_px, height_px)?;

    let side = usize::try_from(MAX_BITMAP_SIDE_PX).unwrap_or(usize::MAX);
    let options = DecoderOptions::default()
        .set_max_width(side)
        .set_max_height(side)
        .set_strict_mode(true)
        .set_use_unsafe(false)
        .jpeg_set_max_scans(JPEG_MAX_SCANS)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().map_err(decode_error)?;
    let decoded_size = decoder
        .info()
        .map(|info| (u32::from(info.width), u32::from(info.height)));
    if decoded_size != Some((width_px, height_px)) {
        return Err(ImportError::ImageDecode {
            reason: format!(
                "JPEG: the decoder read a frame of another size than the {width_px} × {height_px} px header"
            ),
        });
    }
    // Three samples a pixel, at most 3 × 8192² bytes: the side limit
    // bounds it. Reserved first, so a machine that cannot spare it is told
    // so and the process is not aborted.
    let count = decoder
        .output_buffer_size()
        .ok_or_else(|| ImportError::ImageDecode {
            reason: "JPEG: the picture's size overflows".to_owned(),
        })?;
    let mut rgb = Vec::new();
    rgb.try_reserve_exact(count)
        .map_err(|_| ImportError::ImageDecode {
            reason: format!("there is no memory for its {width_px} × {height_px} pixels"),
        })?;
    rgb.resize(count, 0);
    decoder.decode_into(&mut rgb).map_err(decode_error)?;

    // In place: pixel `k`'s gray goes to byte `k`, which is at or before
    // its own samples, so nothing is read after it is overwritten.
    let mut converter = Converter::default();
    let pixels = count / 3;
    for k in 0..pixels {
        let Some(&[r, g, b]) = rgb.get(3 * k..3 * k + 3) else {
            break;
        };
        if let Some(out) = rgb.get_mut(k) {
            *out = converter.pixel(r, g, b, u8::MAX);
        }
    }
    rgb.truncate(pixels);
    rgb.shrink_to_fit();
    Ok(Decoded {
        pixels: Grayscale {
            width_px,
            height_px,
            bytes: rgb,
        },
        sixteen_bit: false,
        seen: converter,
    })
}

/// A decoder error as an [`ImportError`].
fn decode_error(err: zune_jpeg::errors::DecodeErrors) -> ImportError {
    ImportError::ImageDecode {
        reason: format!("JPEG: {err}"),
    }
}

/// The width and height in the JPEG's frame header (its `SOF` marker
/// segment, ITU T.81 B.2.2), found by walking the marker segments from
/// the start of image. Reads lengths and nothing else; no image data is
/// decoded.
fn frame_size(bytes: &[u8]) -> Result<(u32, u32), ImportError> {
    let truncated = || ImportError::ImageDecode {
        reason: "JPEG: the file ends before its frame header".to_owned(),
    };
    let byte = |at: usize| bytes.get(at).copied().ok_or_else(truncated);
    let word = |at: usize| Ok::<_, ImportError>(u16::from_be_bytes([byte(at)?, byte(at + 1)?]));
    // After the start of image, FF D8.
    let mut at = 2;
    loop {
        if byte(at)? != 0xff {
            return Err(ImportError::ImageDecode {
                reason: format!("JPEG: no marker at byte {at}, before the frame header"),
            });
        }
        // Any number of fill bytes may precede a marker.
        while byte(at)? == 0xff {
            at += 1;
        }
        let marker = byte(at)?;
        at += 1;
        match marker {
            // Start of frame: baseline, extended, progressive, lossless,
            // arithmetic, hierarchical. Not C4 (DHT), C8 (JPG) or CC (DAC).
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                // Length, precision, then height and width.
                let height = word(at + 3)?;
                let width = word(at + 5)?;
                return Ok((u32::from(width), u32::from(height)));
            }
            // Markers with no segment: TEM, RSTn, a second SOI.
            0x01 | 0xd0..=0xd8 => {}
            0xd9 | 0xda => {
                return Err(ImportError::ImageDecode {
                    reason: "JPEG: no frame header before the image data".to_owned(),
                });
            }
            _ => {
                let length = usize::from(word(at)?);
                if length < 2 {
                    return Err(ImportError::ImageDecode {
                        reason: format!("JPEG: a marker segment of length {length}"),
                    });
                }
                at += length;
            }
        }
    }
}
