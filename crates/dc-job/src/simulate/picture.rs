// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The finished stock as a picture (phase 6 functional spec, capability
//! 125): a 16-bit grayscale PNG, one pixel a cell, white at the board's
//! top and black at its bottom. Its first row is the grid's last, since
//! a picture's top is the highest Y: the one place the simulation's rows
//! meet a top-first image.
//!
//! The picture says it is DreamCarve's in a `Software` text chunk, so
//! `simulate --out` can write over its own earlier picture and nothing
//! else, as `check --preview` treats its SVG. A `Comment` holds the cell
//! size and the thickness, which make the pixels millimetres again.

use std::io::Cursor;

use dc_sim::Heightfield;

use crate::JobError;

/// What a heightmap picture's `Software` text says.
pub const PICTURE_SOFTWARE: &str = "DreamCarve simulate";

/// The most a pixel can say: white.
const WHITE: f64 = 65_535.0;

/// The bytes of a file read to tell whether it is a heightmap picture:
/// the text chunks come before the pixels, well inside this.
const SNIFF_PIXELS: usize = 1 << 26;

/// `field` as a 16-bit grayscale PNG.
///
/// # Errors
///
/// [`JobError::Heightmap`] when the encoder refuses, which it does only
/// for a picture with no size.
pub fn heightmap_png(field: &Heightfield) -> Result<Vec<u8>, JobError> {
    let grid = field.grid();
    let (nx, ny, top_mm) = (grid.nx() as usize, grid.ny() as usize, field.top_z_mm());
    let heights = field.heights();
    let mut samples = Vec::with_capacity(nx * ny * 2);
    for row in heights.chunks_exact(nx).rev() {
        for &z_mm in row {
            // From 0 at the bottom to 65 535 at the top, both exactly.
            let level = (z_mm / top_mm * WHITE).round().clamp(0.0, WHITE);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a whole number from 0 to 65 535"
            )]
            samples.extend_from_slice(&(level as u16).to_be_bytes());
        }
    }
    let refused = |e: png::EncodingError| JobError::Heightmap {
        reason: e.to_string(),
    };
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, grid.nx(), grid.ny());
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Sixteen);
    encoder
        .add_text_chunk("Software".to_owned(), PICTURE_SOFTWARE.to_owned())
        .map_err(refused)?;
    encoder
        .add_text_chunk(
            "Comment".to_owned(),
            format!(
                "cell_mm={} thickness_mm={} white=top black=bottom",
                grid.cell_mm(),
                top_mm
            ),
        )
        .map_err(refused)?;
    let mut writer = encoder.write_header().map_err(refused)?;
    writer.write_image_data(&samples).map_err(refused)?;
    writer.finish().map_err(refused)?;
    Ok(out)
}

/// Whether `bytes` are a heightmap picture [`heightmap_png`] wrote: a
/// PNG whose `Software` text is [`PICTURE_SOFTWARE`].
#[must_use]
pub fn is_heightmap(bytes: &[u8]) -> bool {
    let limits = png::Limits {
        bytes: SNIFF_PIXELS,
    };
    let decoder = png::Decoder::new_with_limits(Cursor::new(bytes), limits);
    decoder.read_info().is_ok_and(|reader| {
        reader
            .info()
            .uncompressed_latin1_text
            .iter()
            .any(|text| text.keyword == "Software" && text.text == PICTURE_SOFTWARE)
    })
}
