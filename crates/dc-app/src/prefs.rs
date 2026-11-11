// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The preferences record (`docs/specs/phase-4/design.md`, "Preferences";
//! functional spec, open question 11). `dc-app` holds the data; the file,
//! `gui.toml`, is `dreamcarve-shell`'s (`gui_prefs`), whose fields and
//! defaults this record mirrors. View state, never project state: nothing
//! here reaches the `.dcv` or the undo history (acceptance 81).

use std::collections::BTreeMap;
use std::path::PathBuf;

use dc_project::{TabShape, Units};

use crate::view::{ColourMode, Visibility};

/// Everything the app remembers between sessions.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Prefs {
    /// The window's last size and place, when it has been shown.
    pub window: Option<WindowGeometry>,
    /// The side panels and the Warnings drawer.
    pub panels: Panels,
    /// Whether each collapsible section of a panel is open, by its name.
    pub sections: BTreeMap<String, bool>,
    /// What the Viewport draws of a toolpath, as last set.
    pub visibility: Visibility,
    /// How the Viewport colours toolpaths, as last set.
    pub colour: ColourMode,
    /// Recently opened projects, most recent first, at most
    /// [`Prefs::MAX_RECENT`].
    pub recent: Vec<PathBuf>,
    /// Where each project (by its path) last exported, so the next export
    /// starts there.
    pub export_dirs: BTreeMap<PathBuf, PathBuf>,
    /// Light or dark, or the system's choice.
    pub theme: Theme,
    /// The last-used tab size, which the Tab tool adds tabs with
    /// (functional spec, capability 69).
    pub tab: TabSize,
    /// The Tool Library's display units (capability 64): what the sheet
    /// shows, never what the library file stores (millimetres). `None`
    /// until chosen, and then the open project's units.
    pub library_units: Option<Units>,
}

/// A placed tab's width, height and shape, as the Tab tool adds it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabSize {
    /// Length along the outline, mm.
    pub width_mm: f64,
    /// Top above the stock bottom, mm.
    pub height_mm: f64,
    /// Cross-section.
    pub shape: TabShape,
}

impl Default for TabSize {
    /// `init`'s and `tabs place`'s: 8 × 3 mm, square-shouldered.
    fn default() -> Self {
        Self {
            width_mm: 8.0,
            height_mm: 3.0,
            shape: TabShape::Rect,
        }
    }
}

impl Prefs {
    /// How many recent files are kept (functional spec, capability 51).
    pub const MAX_RECENT: usize = 10;

    /// Puts `path` first in the recent list, once, dropping the oldest
    /// past [`Prefs::MAX_RECENT`].
    pub fn push_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| *p != path);
        self.recent.insert(0, path);
        self.recent.truncate(Self::MAX_RECENT);
    }
}

/// The app's appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Follow the operating system.
    #[default]
    System,
    /// The light palette.
    Light,
    /// The dark palette.
    Dark,
}

/// The window's size and place on screen, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
    /// Whether the window was maximized; then the size is the restored one.
    pub maximized: bool,
}

impl Default for WindowGeometry {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 800.0,
            maximized: false,
        }
    }
}

/// The side panels' widths and whether each is open, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Panels {
    /// The left panel's width.
    pub left_width: f64,
    /// Whether the left panel is open.
    pub left_open: bool,
    /// The right panel's width.
    pub right_width: f64,
    /// Whether the right panel is open.
    pub right_open: bool,
    /// Whether the Warnings drawer is open.
    pub warnings_open: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            left_width: 280.0,
            left_open: true,
            right_width: 320.0,
            right_open: true,
            warnings_open: false,
        }
    }
}
