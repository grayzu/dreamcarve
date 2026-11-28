// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The preferences record (`docs/specs/phase-4/design.md`, "Preferences";
//! functional spec, open question 11). `dc-app` holds the data; the file,
//! `gui.toml`, is `dreamcarve-shell`'s (`gui_prefs`), whose fields and
//! defaults this record mirrors. View state, never project state: nothing
//! here reaches the `.dcv` or the undo history (acceptance 81).

use std::collections::BTreeMap;
use std::path::PathBuf;

use dc_project::{Measured, PostProcessorRef, TabShape, Units};

use crate::snap::SnapSettings;
use crate::tool::{CornerStyle, Drawing};
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
    /// Snapping (phase 5, capability 82): the master switch, the kinds
    /// that are on and the radius.
    pub snap: SnapSettings,
    /// The snap grid's spacing when the user has pinned one, mm
    /// (capability 82a); `None` is automatic, following the zoom.
    pub grid_spacing_mm: Option<f64>,
    /// The drawing tool the Draw button holds: the last one armed
    /// (capability 81).
    pub drawing_tool: Drawing,
    /// The point of a selection's box that a resize, a turn and a flip
    /// are made about ([`crate::origin`]); the centre until another is
    /// chosen. Kept while the app runs; not written to `gui.toml`.
    pub origin: crate::origin::Origin,
    /// The tool each group of drawing tools holds: the one of it last
    /// armed. Kept while the app runs; not written to `gui.toml`.
    pub held_tools: crate::tool::HeldTools,
    /// The drawing tools' options, as last set (capabilities 85 and 85b).
    pub drawing: DrawingPrefs,
    /// What the Shape menu's Offset…, Fillet Corners… and Join were last
    /// given. Kept while the app runs; not written to `gui.toml`.
    pub adjust: crate::tool::AdjustPrefs,
    /// What the Text tool sets a text in, as last set (capability 91).
    pub text: TextPrefs,
    /// The Simulate step's display and quality, as last set (phase 6,
    /// capability 124).
    pub sim: crate::sim::SimPrefs,
    /// The controller and output settings a new project starts with, once
    /// the user has chosen them (phase 4, capability 51a); `None` is
    /// grblHAL at its latest version with its own defaults. The one
    /// preference that reaches a project, and only a new one: what is
    /// already open or saved never follows it.
    pub machine: Option<PostProcessorRef>,
}

/// What the Text tool remembers between texts: the font, the height,
/// the spacings and the alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct TextPrefs {
    /// The font; the first font on hand when this one is not.
    pub font: dc_project::FontRef,
    /// The height of a capital letter, mm; `None` until one is typed,
    /// and then 10 mm, or 0.4 in in an inch project.
    pub height_mm: Option<f64>,
    /// Extra space between letters, a share of the height.
    pub letter_spacing_fraction: f64,
    /// The distance between lines, a share of the font's own.
    pub line_spacing_fraction: f64,
    /// How several lines are set.
    pub align: dc_project::Align,
}

impl Default for TextPrefs {
    /// Inter, the font's own spacings, set left.
    fn default() -> Self {
        Self {
            font: dc_project::FontRef {
                family: "Inter".to_owned(),
                style: "Regular".to_owned(),
            },
            height_mm: None,
            letter_spacing_fraction: 0.0,
            line_spacing_fraction: 1.0,
            align: dc_project::Align::Left,
        }
    }
}

/// A new rectangle's corner size in a millimetre project, mm
/// (PureCutCNC's default).
const CORNER_MM: f64 = 5.0;
/// A new rectangle's corner size in an inch project, mm: 0.2 in.
const CORNER_INCH_MM: f64 = 5.08;

/// What the drawing tools remember between shapes and sessions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawingPrefs {
    /// The Rectangle tool's corners.
    pub corners: CornerStyle,
    /// The size of a rounded or chamfered corner, mm; `None` until it is
    /// typed, and then [`DrawingPrefs::corner_size_mm`]'s default.
    pub corner_mm: Option<f64>,
    /// The Polygon tool's sides.
    pub polygon_sides: u32,
    /// What the Polygon tool's radius reaches.
    pub polygon_measured: Measured,
}

impl Default for DrawingPrefs {
    /// Square corners, six sides, measured to the corners.
    fn default() -> Self {
        Self {
            corners: CornerStyle::Square,
            corner_mm: None,
            polygon_sides: 6,
            polygon_measured: Measured::Corners,
        }
    }
}

impl DrawingPrefs {
    /// The corner size the Rectangle tool uses in a project of `units`:
    /// the one typed, else [`default_corner_mm`].
    #[must_use]
    pub fn corner_size_mm(&self, units: Units) -> f64 {
        self.corner_mm.unwrap_or_else(|| default_corner_mm(units))
    }
}

/// The size a rounded or chamfered corner has until one is typed, in a
/// project of `units`: 5 mm or 0.2 in (PureCutCNC's).
#[must_use]
pub const fn default_corner_mm(units: Units) -> f64 {
    match units {
        Units::Mm => CORNER_MM,
        Units::Inch => CORNER_INCH_MM,
    }
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

    /// The size the user dragged the sheet to, a side at a time, held
    /// as [`SheetSize::of`] holds it in a window of `window_px`; `None`
    /// for a side never dragged. A sheet that is as tall as its content
    /// until then reads this (the Import drawing sheet), and keeps each
    /// side with [`SheetSize::keep`].
    #[must_use]
    pub fn kept(self, prefs: &Prefs, window_px: (f64, f64)) -> (Option<f64>, Option<f64>) {
        let kept = |axis: &str| {
            prefs
                .sizes
                .get(&format!("{}.{axis}", self.key))
                .copied()
                .filter(|px| px.is_finite())
        };
        let (width, height) = (kept("width"), kept("height"));
        let held = self.held(
            (
                width.unwrap_or(self.default_px.0),
                height.unwrap_or(self.default_px.1),
            ),
            window_px,
        );
        (width.map(|_| held.0), height.map(|_| held.1))
    }

    /// Keeps each side of `size` that is given as the sheet's in `prefs`,
    /// held as [`SheetSize::of`] holds it, and leaves the other as it
    /// was.
    pub fn keep(self, prefs: &mut Prefs, size: (Option<f64>, Option<f64>), window_px: (f64, f64)) {
        let held = self.held(
            (
                size.0.unwrap_or(self.default_px.0),
                size.1.unwrap_or(self.default_px.1),
            ),
            window_px,
        );
        if size.0.is_some() {
            prefs.sizes.insert(format!("{}.width", self.key), held.0);
        }
        if size.1.is_some() {
            prefs.sizes.insert(format!("{}.height", self.key), held.1);
        }
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

    /// The post-processor reference a new project gets (capability 51a):
    /// the chosen machine at the latest version of its definition, with
    /// the settings that version can use ([`dc_project::switch_post`]),
    /// or, when none is chosen or it no longer resolves (a `gui.toml` a
    /// newer DreamCarve wrote), grblHAL at its latest with its defaults.
    ///
    /// # Errors
    ///
    /// [`crate::CommandError::PostProcessor`] if the bundled grblHAL
    /// definition cannot be read, which no release ships.
    pub fn new_project_post(&self) -> Result<PostProcessorRef, crate::CommandError> {
        if let Some(machine) = &self.machine
            && let Ok((post, _)) = dc_project::switch_post(machine, &machine.id)
        {
            return Ok(post);
        }
        Ok(dc_project::latest_definition(DEFAULT_POST_ID)?.default_ref())
    }

    /// Whether `post` is the machine new projects start with: the same
    /// controller and settings, whatever version of its definition each
    /// names (a new project takes the latest).
    #[must_use]
    pub fn is_new_project_machine(&self, post: &PostProcessorRef) -> bool {
        self.machine.as_ref().is_some_and(|machine| {
            PostProcessorRef {
                definition_version: post.definition_version,
                ..machine.clone()
            } == *post
        })
    }
}

/// The bundled post-processor a new project names until the user chooses
/// another (capability 51).
pub const DEFAULT_POST_ID: &str = "grblhal";

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
