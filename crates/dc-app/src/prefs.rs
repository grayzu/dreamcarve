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
    /// Dark, light, or the system's choice; dark until chosen.
    pub theme: Theme,
    /// The last-used tab size, which the Tab tool adds tabs with
    /// (functional spec, capability 69).
    pub tab: TabSize,
    /// The Tool Library's display units (capability 64): what the sheet
    /// shows, never what the library file stores (millimetres). `None`
    /// until chosen, and then the open project's units.
    pub library_units: Option<Units>,
    /// The sizes the user has dragged things to, in logical pixels, by
    /// name: a sheet that resizes (`library.width`, `library.height`) and
    /// the Tool Library table's columns (`library.column.type`). A name
    /// that is missing is the default.
    pub sizes: BTreeMap<String, f64>,
}

/// What a sheet keeps clear of the window's edges on each axis, px: 24
/// either side.
pub const SHEET_GAP_PX: f64 = 48.0;

/// A sheet the user can resize: its name in [`Prefs::sizes`], and its
/// default and smallest sizes, px (width, height).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheetSize {
    /// Its name: `<key>.width` and `<key>.height` hold its size.
    pub key: &'static str,
    /// The size it opens at until it is dragged.
    pub default_px: (f64, f64),
    /// The smallest it goes.
    pub min_px: (f64, f64),
}

impl SheetSize {
    /// `size` held between the minimum and what a window of `window_px`
    /// leaves round the sheet; in a window smaller than the minimum, what
    /// the window leaves.
    fn held(self, size: (f64, f64), window_px: (f64, f64)) -> (f64, f64) {
        let hold = |value: f64, min: f64, window: f64| {
            let most = (window - SHEET_GAP_PX).max(0.0);
            value.max(min).min(most)
        };
        (
            hold(size.0, self.min_px.0, window_px.0),
            hold(size.1, self.min_px.1, window_px.1),
        )
    }

    /// The sheet's size in a window of `window_px`: the one kept in
    /// `prefs`, else the default, held inside the window.
    #[must_use]
    pub fn of(self, prefs: &Prefs, window_px: (f64, f64)) -> (f64, f64) {
        let kept = |axis: &str, default: f64| {
            prefs
                .sizes
                .get(&format!("{}.{axis}", self.key))
                .copied()
                .filter(|px| px.is_finite())
                .unwrap_or(default)
        };
        self.held(
            (
                kept("width", self.default_px.0),
                kept("height", self.default_px.1),
            ),
            window_px,
        )
    }

    /// Keeps `size` as the sheet's in `prefs`, held as [`SheetSize::of`]
    /// holds it.
    pub fn set(self, prefs: &mut Prefs, size: (f64, f64), window_px: (f64, f64)) {
        let (width, height) = self.held(size, window_px);
        prefs.sizes.insert(format!("{}.width", self.key), width);
        prefs.sizes.insert(format!("{}.height", self.key), height);
    }
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

/// The app's appearance. Dark until the user chooses (the redesign,
/// `docs/specs/phase-4/redesign.md`, principle 5: dark first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    /// Follow the operating system.
    System,
    /// The light palette.
    Light,
    /// The dark palette, the default.
    #[default]
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

/// The step panel's and the Inspector's widths, in logical pixels, and
/// whether the step panel shows. `left` is the step panel and `right` the
/// Inspector, which shows whenever its step has something selected
/// (`redesign.md`, "The window").
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
            left_width: 312.0,
            left_open: true,
            right_width: 328.0,
            right_open: true,
            warnings_open: false,
        }
    }
}
