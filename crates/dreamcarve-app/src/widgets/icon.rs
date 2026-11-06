// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Lucide icons the shell draws (`docs/specs/phase-4/experience.md`,
//! section 6), by name and code point in the bundled `lucide.ttf`
//! (`assets/icons/lucide/codepoints.json`, lucide-static 1.67.0).

use eframe::egui::{self, FontFamily, FontId, Galley};

use crate::shell::theme;

/// An icon of the set, named as Lucide names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// `folder-open`: Open.
    FolderOpen,
    /// `save`: Save.
    Save,
    /// `undo-2`: Undo.
    Undo,
    /// `redo-2`: Redo.
    Redo,
    /// `file-input`: Import a drawing.
    FileInput,
    /// `image-plus`: Import a picture.
    ImagePlus,
    /// `maximize`: Fit.
    Maximize,
    /// `bookmark-plus`: the Tab tool.
    BookmarkPlus,
    /// `download`: Export.
    Download,
    /// `x`: close.
    X,
    /// `circle-x`: an error.
    CircleX,
    /// `triangle-alert`: a warning.
    TriangleAlert,
    /// `panel-left-open`: reopen the left column.
    PanelLeftOpen,
    /// `panel-right-open`: reopen the CAM panel.
    PanelRightOpen,
    /// `info`: an unused tool's hint.
    Info,
    /// `search`: the library's search field.
    Search,
    /// `check`: a row already in the project.
    Check,
    /// `plus`: Add from library.
    Plus,
    /// `book-plus`: Save to library.
    BookPlus,
    /// `arrow-left`: Back.
    ArrowLeft,
    /// `lock`: a locked shape's padlock on the Viewport.
    Lock,
    /// `eye`: a shown row.
    Eye,
    /// `eye-off`: a hidden row.
    EyeOff,
    /// `lock-open`: an unlocked row on hover; the W/H padlock open.
    LockOpen,
    /// `link-2`: a linked copy.
    Link2,
    /// `bookmark`: a tab row.
    Bookmark,
    /// `flip-vertical-2`, mirrored about a vertical line: Mirror ↔ (Lucide
    /// names a flip by its axis).
    MirrorLeftRight,
    /// `flip-horizontal-2`, mirrored about a horizontal line: Mirror ↕.
    MirrorTopBottom,
}

impl Icon {
    /// Lucide's name.
    #[cfg(test)]
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::FolderOpen => "folder-open",
            Self::Save => "save",
            Self::Undo => "undo-2",
            Self::Redo => "redo-2",
            Self::FileInput => "file-input",
            Self::ImagePlus => "image-plus",
            Self::Maximize => "maximize",
            Self::BookmarkPlus => "bookmark-plus",
            Self::Download => "download",
            Self::X => "x",
            Self::CircleX => "circle-x",
            Self::TriangleAlert => "triangle-alert",
            Self::PanelLeftOpen => "panel-left-open",
            Self::PanelRightOpen => "panel-right-open",
            Self::Info => "info",
            Self::Search => "search",
            Self::Check => "check",
            Self::Plus => "plus",
            Self::BookPlus => "book-plus",
            Self::ArrowLeft => "arrow-left",
            Self::Lock => "lock",
            Self::Eye => "eye",
            Self::EyeOff => "eye-off",
            Self::LockOpen => "lock-open",
            Self::Link2 => "link-2",
            Self::Bookmark => "bookmark",
            Self::MirrorLeftRight => "flip-vertical-2",
            Self::MirrorTopBottom => "flip-horizontal-2",
        }
    }

    /// The glyph's code point in `lucide.ttf`.
    #[must_use]
    pub const fn codepoint(self) -> char {
        match self {
            Self::FolderOpen => '\u{e247}',
            Self::Save => '\u{e14d}',
            Self::Undo => '\u{e2a1}',
            Self::Redo => '\u{e2a0}',
            Self::FileInput => '\u{e0c5}',
            Self::ImagePlus => '\u{e1f7}',
            Self::Maximize => '\u{e112}',
            Self::BookmarkPlus => '\u{e23d}',
            Self::Download => '\u{e0b2}',
            Self::X => '\u{e1b2}',
            Self::CircleX => '\u{e084}',
            Self::TriangleAlert => '\u{e193}',
            Self::PanelLeftOpen => '\u{e21d}',
            Self::PanelRightOpen => '\u{e434}',
            Self::Info => '\u{e0f9}',
            Self::Search => '\u{e151}',
            Self::Check => '\u{e06c}',
            Self::Plus => '\u{e13d}',
            Self::BookPlus => '\u{e3f3}',
            Self::ArrowLeft => '\u{e048}',
            Self::Lock => '\u{e10b}',
            Self::Eye => '\u{e0ba}',
            Self::EyeOff => '\u{e0bb}',
            Self::LockOpen => '\u{e10c}',
            Self::Link2 => '\u{e103}',
            Self::Bookmark => '\u{e060}',
            Self::MirrorLeftRight => '\u{e360}',
            Self::MirrorTopBottom => '\u{e35e}',
        }
    }

    /// Every icon, for checking the code points.
    #[cfg(test)]
    pub const ALL: [Self; 28] = [
        Self::FolderOpen,
        Self::Save,
        Self::Undo,
        Self::Redo,
        Self::FileInput,
        Self::ImagePlus,
        Self::Maximize,
        Self::BookmarkPlus,
        Self::Download,
        Self::X,
        Self::CircleX,
        Self::TriangleAlert,
        Self::PanelLeftOpen,
        Self::PanelRightOpen,
        Self::Info,
        Self::Search,
        Self::Check,
        Self::Plus,
        Self::BookPlus,
        Self::ArrowLeft,
        Self::Lock,
        Self::Eye,
        Self::EyeOff,
        Self::LockOpen,
        Self::Link2,
        Self::Bookmark,
        Self::MirrorLeftRight,
        Self::MirrorTopBottom,
    ];

    /// The glyph laid out at `size_px` in `colour`, ready to paint.
    #[must_use]
    pub fn galley(
        self,
        ui: &egui::Ui,
        size_px: f32,
        colour: egui::Color32,
    ) -> std::sync::Arc<Galley> {
        ui.painter().layout_no_wrap(
            self.codepoint().to_string(),
            FontId::new(size_px, FontFamily::Name(theme::ICONS.into())),
            colour,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each icon's code point is the one the bundled font's map gives its
    /// name, so a font update that renumbers the set fails here.
    #[test]
    fn codepoints_match_the_bundled_map() {
        let map = include_str!("../../assets/icons/lucide/codepoints.json");
        for icon in Icon::ALL {
            let entry = format!("\"{}\": {},", icon.name(), u32::from(icon.codepoint()));
            let last = format!("\"{}\": {}\n", icon.name(), u32::from(icon.codepoint()));
            assert!(
                map.contains(&entry) || map.contains(&last),
                "{} is not {:#x} in codepoints.json",
                icon.name(),
                u32::from(icon.codepoint())
            );
        }
    }
}
