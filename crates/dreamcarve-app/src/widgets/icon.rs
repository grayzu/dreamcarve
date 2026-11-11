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
    /// `chevron-right`: a closed section.
    ChevronRight,
    /// `chevron-down`: an open section, a dropdown.
    ChevronDown,
    /// `crosshair`: pick a point on the Viewport.
    Crosshair,
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
    /// `lock-open`: an unlocked row on hover.
    LockOpen,
    /// `component`: a linked copy (not a chain: the chain is the
    /// proportion link).
    Component,
    /// `link`: W and H keep their proportions.
    Link,
    /// `unlink`: W and H scale on their own.
    Unlink,
    /// `square`: a closed outline's row.
    Square,
    /// `circle`: a circle's row.
    Circle,
    /// `spline`: an open path's row.
    Spline,
    /// `rotate-cw-square`: the toolbar's Rotate.
    RotateCwSquare,
    /// `bookmark`: a tab row.
    Bookmark,
    /// Flip horizontal (left to right). Lucide's `flip-vertical-2`: Lucide
    /// names a flip by its dashed axis, which here is vertical, so the
    /// glyph pictures a left-to-right mirror as the mock-up draws it.
    FlipHorizontal,
    /// Flip vertical (top to bottom): Lucide's `flip-horizontal-2`, the
    /// mirror about a horizontal dashed line.
    FlipVertical,
    /// `grip-vertical`: a list row's drag handle.
    GripVertical,
    /// `rotate-ccw`: reset a field to the tool's value.
    RotateCcw,
    /// `copy`: Duplicate an operation.
    Copy,
    /// `trash-2`: Delete an operation.
    Trash2,
    /// `cylinder`: an end mill.
    Cylinder,
    /// `circle-dot`: a ball-nose bit.
    CircleDot,
    /// `triangle`: a V-bit (drawn point down).
    Triangle,
    /// `pencil-line`: an engraving bit.
    PencilLine,
    /// `move-down`: a drill.
    MoveDown,
    /// `layers`: the status bar's Toolpaths button.
    Layers,
    /// `circle-check`: the status bar's *No problems*.
    CircleCheck,
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
            Self::ChevronRight => "chevron-right",
            Self::ChevronDown => "chevron-down",
            Self::Crosshair => "crosshair",
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
            Self::Component => "component",
            Self::Link => "link",
            Self::Unlink => "unlink",
            Self::Square => "square",
            Self::Circle => "circle",
            Self::Spline => "spline",
            Self::RotateCwSquare => "rotate-cw-square",
            Self::Bookmark => "bookmark",
            Self::FlipHorizontal => "flip-vertical-2",
            Self::FlipVertical => "flip-horizontal-2",
            Self::GripVertical => "grip-vertical",
            Self::RotateCcw => "rotate-ccw",
            Self::Copy => "copy",
            Self::Trash2 => "trash-2",
            Self::Cylinder => "cylinder",
            Self::CircleDot => "circle-dot",
            Self::Triangle => "triangle",
            Self::PencilLine => "pencil-line",
            Self::MoveDown => "move-down",
            Self::Layers => "layers",
            Self::CircleCheck => "circle-check",
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
            Self::ChevronRight => '\u{e06f}',
            Self::ChevronDown => '\u{e06d}',
            Self::Crosshair => '\u{e0ac}',
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
            Self::Component => '\u{e2ad}',
            Self::Link => '\u{e102}',
            Self::Unlink => '\u{e19c}',
            Self::Square => '\u{e167}',
            Self::Circle => '\u{e076}',
            Self::Spline => '\u{e38b}',
            Self::RotateCwSquare => '\u{e5d1}',
            Self::Bookmark => '\u{e060}',
            Self::FlipHorizontal => '\u{e360}',
            Self::FlipVertical => '\u{e35e}',
            Self::GripVertical => '\u{e0eb}',
            Self::RotateCcw => '\u{e148}',
            Self::Copy => '\u{e09e}',
            Self::Trash2 => '\u{e18e}',
            Self::Cylinder => '\u{e525}',
            Self::CircleDot => '\u{e345}',
            Self::Triangle => '\u{e192}',
            Self::PencilLine => '\u{e4f0}',
            Self::MoveDown => '\u{e48c}',
            Self::Layers => '\u{e529}',
            Self::CircleCheck => '\u{e226}',
        }
    }

    /// Every icon, for checking the code points.
    #[cfg(test)]
    pub const ALL: [Self; 48] = [
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
        Self::ChevronRight,
        Self::ChevronDown,
        Self::Crosshair,
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
        Self::Component,
        Self::Link,
        Self::Unlink,
        Self::Square,
        Self::Circle,
        Self::Spline,
        Self::RotateCwSquare,
        Self::Bookmark,
        Self::FlipHorizontal,
        Self::FlipVertical,
        Self::GripVertical,
        Self::RotateCcw,
        Self::Copy,
        Self::Trash2,
        Self::Cylinder,
        Self::CircleDot,
        Self::Triangle,
        Self::PencilLine,
        Self::MoveDown,
        Self::Layers,
        Self::CircleCheck,
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

/// A dropdown's chevron (`experience.md`, section 7, "Dropdown"): Lucide
/// `chevron-down` at 16 px in the control's text colour, painted into the
/// icon slot `ComboBox::icon` gives, in place of egui's filled triangle
/// (#549).
pub(crate) fn combo_chevron(
    ui: &egui::Ui,
    rect: egui::Rect,
    visuals: &egui::style::WidgetVisuals,
    _open: bool,
) {
    let colour = visuals.fg_stroke.color;
    let galley = Icon::ChevronDown.galley(ui, theme::ICON_SMALL_PX, colour);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
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
