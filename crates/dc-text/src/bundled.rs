// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The fonts that travel with the app (phase 5 design, "Bundled
//! fonts"), compiled in from `fonts/`: no file is read at run time. Each
//! is as its publisher released it, neither renamed nor modified, with
//! its licence beside it; `fonts/README.md` gives each file's source and
//! checksum.

use crate::font::Font;

/// One bundled font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bundled {
    /// The family, as the font names itself.
    pub family: &'static str,
    /// The style, as the font names itself.
    pub style: &'static str,
    /// Its licence, as an SPDX identifier.
    pub licence: &'static str,
    /// The font file; `None` for the single-line font, which is a table
    /// in this crate.
    bytes: Option<&'static [u8]>,
}

/// The licence of the fonts under the SIL Open Font License 1.1.
const OFL: &str = "OFL-1.1";
/// The licence of Roboto Slab, and of the single-line font, which is
/// PureCutCNC's.
const APACHE: &str = "Apache-2.0";

/// The bundled fonts, in the order the Font list offers them.
const BUNDLED: &[Bundled] = &[
    Bundled {
        family: "Inter",
        style: "Regular",
        licence: OFL,
        bytes: Some(include_bytes!("../fonts/inter/Inter-Regular.ttf")),
    },
    Bundled {
        family: "Inter",
        style: "Bold",
        licence: OFL,
        bytes: Some(include_bytes!("../fonts/inter/Inter-Bold.ttf")),
    },
    Bundled {
        family: "Source Serif 4",
        style: "Regular",
        licence: OFL,
        bytes: Some(include_bytes!(
            "../fonts/source-serif-4/SourceSerif4-Regular.ttf"
        )),
    },
    Bundled {
        family: "Source Serif 4",
        style: "Bold",
        licence: OFL,
        bytes: Some(include_bytes!(
            "../fonts/source-serif-4/SourceSerif4-Bold.ttf"
        )),
    },
    Bundled {
        family: "Roboto Slab",
        style: "Bold",
        licence: APACHE,
        bytes: Some(include_bytes!("../fonts/roboto-slab/RobotoSlab-Bold.ttf")),
    },
    Bundled {
        family: "Pacifico",
        style: "Regular",
        licence: OFL,
        bytes: Some(include_bytes!("../fonts/pacifico/Pacifico-Regular.ttf")),
    },
    Bundled {
        family: "Allerta Stencil",
        style: "Regular",
        licence: OFL,
        bytes: Some(include_bytes!(
            "../fonts/allerta-stencil/AllertaStencil-Regular.ttf"
        )),
    },
    Bundled {
        family: crate::font::SIMPLE_STROKE_FAMILY,
        style: crate::font::SIMPLE_STROKE_STYLE,
        licence: APACHE,
        bytes: None,
    },
];

/// The bundled fonts.
#[must_use]
pub const fn bundled() -> &'static [Bundled] {
    BUNDLED
}

/// The bundled font of `family` and `style`, ready to lay text out in;
/// `None` when no bundled font is so named.
#[must_use]
pub fn bundled_font(family: &str, style: &str) -> Option<Font> {
    let found = BUNDLED
        .iter()
        .find(|font| font.family == family && font.style == style)?;
    match found.bytes {
        Some(bytes) => Font::of_static(bytes).ok(),
        None => Some(Font::simple_stroke()),
    }
}
