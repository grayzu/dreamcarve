// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The app's preferences file, `<config dir>/gui.toml`
//! (`docs/specs/phase-4/design.md`, "Preferences"; functional spec, open
//! question 11): the window and panel layout, the toolpath display
//! settings, the recent files, where each project last exported, the
//! theme, the Tool Library's display units, the sizes the user dragged
//! a sheet or a table's columns to, and the snapping and drawing-tool
//! settings. View state, never project state: nothing here changes a program
//! or reaches the `.dcv`.
//!
//! Read at start and written on change and on exit. A missing file is the
//! defaults. Unlike `config.toml`, a file that does not parse, or has a key
//! this build does not know (an older or newer DreamCarve wrote it), is
//! the defaults plus a one-line note for the Warnings panel, never a
//! refusal to start: a preference is not worth losing the session over.
//! A file that is there and could not be used at all is reported as such
//! (`Loaded::unread`), so the app can say so and leave it alone: it may
//! be one typo away from the user's settings, and writing the defaults
//! over it would lose them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::line_column;
use crate::config_dir::Paths;
use crate::error::ShellError;
use crate::staged::StagedFile;

/// Everything `gui.toml` holds. Every key is optional and defaults when
/// missing.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GuiPrefs {
    /// Dark, light, or the system's choice; dark when the key is missing.
    pub theme: Theme,
    /// The window's last size and place, when it has been shown.
    pub window: Option<WindowGeometry>,
    /// The side panels and the Warnings drawer.
    pub panels: Panels,
    /// Whether each collapsible section of a panel is open, by its name.
    pub sections: BTreeMap<String, bool>,
    /// What the Viewport draws of a toolpath.
    pub visibility: Visibility,
    /// How the Viewport colours toolpaths.
    pub colour: ColourMode,
    /// Recently opened projects, most recent first.
    pub recent: Vec<PathBuf>,
    /// Where each project (by its path) last exported, so the next export
    /// starts there.
    pub export_dirs: BTreeMap<String, PathBuf>,
    /// The last-used tab size, which the Tab tool adds tabs with.
    pub tab: TabSize,
    /// The Tool Library's display units, once chosen; the library file
    /// itself is always millimetres.
    pub library_units: Option<LibraryUnits>,
    /// The sizes the user has dragged things to, in logical pixels, by
    /// name: a sheet that resizes (`library.width`) and the Tool Library
    /// table's columns (`library.column.type`).
    pub sizes: BTreeMap<String, f64>,
    /// Snapping while drawing and editing (phase 5, capability 82).
    pub snap: Snap,
    /// What the drawing tools remember (phase 5, capabilities 81 and
    /// 82a).
    pub drawing: Drawing,
    /// What the Text tool sets a text in (phase 5, capability 91).
    pub text: Text,
}

/// What the Text tool remembers between sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Text {
    /// The font's family, as the font names itself. A font that is not
    /// on hand when the app starts gives way to the first that is.
    pub family: String,
    /// The font's style: `Regular`, `Bold`.
    pub style: String,
    /// The height of a capital letter, mm; missing until one is typed,
    /// and then 10 mm, or 0.4 in in an inch project.
    pub height_mm: Option<f64>,
    /// Extra space between letters, as a fraction of the height.
    pub letter_spacing: f64,
    /// The distance between lines, as a fraction of the font's own.
    pub line_spacing: f64,
    /// How several lines are set: `left`, `centre` or `right`.
    pub align: String,
}

impl Default for Text {
    fn default() -> Self {
        Self {
            family: "Inter".to_owned(),
            style: "Regular".to_owned(),
            height_mm: None,
            letter_spacing: 0.0,
            line_spacing: 1.0,
            align: "left".to_owned(),
        }
    }
}

/// Snapping: the master switch, the kinds that are on, and how near the
/// pointer a candidate must be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snap {
    /// The master switch.
    pub enabled: bool,
    /// The kinds that are on, by name: `endpoint`, `midpoint`, `centre`,
    /// `intersection`, `on_outline`, `perpendicular`, `grid`. A name this
    /// build does not know is left out.
    pub kinds: Vec<String>,
    /// The snap radius, logical pixels.
    pub radius_px: f64,
    /// Whether a point near the horizontal or the vertical through the
    /// last is put on it. Apart from `kinds`, so a file saved before
    /// there was such a snap has it on, as a new one does.
    pub axis: bool,
}

impl Default for Snap {
    /// Every kind but `perpendicular`, at 14 px.
    fn default() -> Self {
        Self {
            enabled: true,
            kinds: [
                "endpoint",
                "midpoint",
                "centre",
                "intersection",
                "on_outline",
                "grid",
            ]
            .map(str::to_owned)
            .to_vec(),
            radius_px: 14.0,
            axis: true,
        }
    }
}

/// What the drawing tools remember between sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Drawing {
    /// The drawing tool the Draw button holds, by name: `line`, `arc`,
    /// `spline`, `rectangle`, `circle`, `ellipse`, `polygon`, `slot`,
    /// `gear`, `text`. A name this build does not know is the default.
    pub tool: String,
    /// The snap grid's spacing when the user has pinned one, mm; missing
    /// for a grid that follows the zoom.
    pub grid_spacing_mm: Option<f64>,
    /// The Rectangle tool's corners: `square`, `rounded` or `chamfered`.
    pub corners: String,
    /// The size of a rounded or chamfered corner, mm; missing until one
    /// is typed, and then 5 mm, or 0.2 in in an inch project.
    pub corner_mm: Option<f64>,
    /// The Polygon tool's sides, 3 to 50.
    pub polygon_sides: u32,
    /// What the Polygon tool's radius reaches: `corners` or `flats`.
    pub polygon_measured: String,
}

impl Default for Drawing {
    fn default() -> Self {
        Self {
            tool: "rectangle".to_owned(),
            grid_spacing_mm: None,
            corners: "square".to_owned(),
            corner_mm: None,
            polygon_sides: 6,
            polygon_measured: "corners".to_owned(),
        }
    }
}

/// The Tool Library's display units (phase 4, capability 64).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryUnits {
    /// Millimetres.
    Mm,
    /// Inches.
    In,
}

/// A placed tab's width, height and shape, as the Tab tool adds it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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

/// A tab's cross-section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabShape {
    /// Square-shouldered.
    #[default]
    Rect,
    /// A smooth ramp up and down.
    Smooth,
}

/// The app's appearance. Dark while `gui.toml` has no `theme` key (the
/// redesign, `docs/specs/phase-4/redesign.md`, principle 5: dark first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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

/// What the Viewport draws of a toolpath. Everything but the arrows, the
/// extents and *Only the selected operation* is on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Visibility {
    /// Cutting moves.
    pub cuts: bool,
    /// Rapid moves.
    pub rapids: bool,
    /// Plunges.
    pub plunges: bool,
    /// Direction arrows along the cuts.
    pub arrows: bool,
    /// Where each descent starts.
    pub start_points: bool,
    /// The stretches over tabs.
    pub tab_marks: bool,
    /// The grid.
    pub grid: bool,
    /// Each program's extents box.
    pub extents: bool,
    /// Only the selected operation's toolpath.
    pub only_selected: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self {
            cuts: true,
            rapids: true,
            plunges: true,
            arrows: false,
            start_points: true,
            tab_marks: true,
            grid: true,
            extents: false,
            only_selected: false,
        }
    }
}

/// How the Viewport colours toolpaths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourMode {
    /// One colour per operation, the preview's palette.
    #[default]
    ByOperation,
    /// By cutting depth.
    ByDepth,
}

/// What [`load`] read: the preferences, and the notes about what it could
/// not read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Loaded {
    /// The preferences, defaults where the file had none or had something
    /// unreadable.
    pub prefs: GuiPrefs,
    /// One line per problem with the file, for the Warnings panel: a key
    /// this build does not know, or a file that did not parse.
    pub notes: Vec<String>,
    /// Why the file is there and none of it is in use: it could not be
    /// read, or it did not parse (a syntax error, a value of the wrong
    /// kind), as `path:line:column: message`, the note of it. `prefs` are
    /// then the defaults, and the caller must not write them over the
    /// file until the user changes a preference. `None` for a missing
    /// file and for one that was read, unknown keys or not.
    pub unread: Option<String>,
}

impl Loaded {
    /// The defaults for a file that could not be used, for the reason
    /// `why`.
    fn unread(why: String) -> Self {
        Self {
            prefs: GuiPrefs::default(),
            notes: vec![why.clone()],
            unread: Some(why),
        }
    }
}

/// Reads `gui.toml` of `paths`. A missing file is the defaults with no
/// note; a file that cannot be read or parsed is the defaults with a note
/// saying why.
#[must_use]
pub fn load(paths: &Paths) -> Loaded {
    let path = &paths.gui_prefs_file;
    match crate::read_capped_text(path, crate::MAX_FILE_BYTES) {
        Ok(text) => parse(&text, path),
        Err(crate::ShellError::Read { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            Loaded::default()
        }
        // A read error's message is `cannot read <path>: <why>`, the note
        // as it always was; a file past the size limit says so the same
        // way.
        Err(e) => Loaded::unread(e.to_string()),
    }
}

/// Parses `text`, the contents of `gui.toml` at `path`. A key this build
/// does not know is left out with a note naming it, at its line and
/// column; a value of the wrong kind, or a syntax error, is the defaults
/// with a note.
#[must_use]
pub fn parse(text: &str, path: &Path) -> Loaded {
    let mut notes = Vec::new();
    let table: toml::Table = match toml::from_str(text) {
        Ok(table) => table,
        Err(e) => return Loaded::unread(describe_error(text, path, &e)),
    };
    for key in unknown_keys(&table) {
        let at = key_position(text, &key)
            .map_or_else(String::new, |(line, column)| format!(":{line}:{column}"));
        notes.push(format!(
            "{}{at}: unknown key `{key}` ignored; a different DreamCarve wrote it",
            path.display()
        ));
    }
    let (prefs, unread) = match table.try_into::<GuiPrefs>() {
        Ok(prefs) => (prefs, None),
        Err(e) => {
            let why = describe_error(text, path, &e);
            notes.push(why.clone());
            (GuiPrefs::default(), Some(why))
        }
    };
    Loaded {
        prefs,
        notes,
        unread,
    }
}

/// A TOML error as `path:line:column: message`, as `config.toml`'s are
/// reported (#76).
fn describe_error(text: &str, path: &Path, e: &toml::de::Error) -> String {
    match e.span() {
        Some(span) => {
            let (line, column) = line_column(text, span.start);
            format!("{}:{line}:{column}: {}", path.display(), e.message())
        }
        None => format!("{}: {}", path.display(), e.message()),
    }
}

/// The keys of `table` this build does not know, as dotted paths
/// (`panels.left_width_px`), in file order. The free maps (`sections`,
/// `export_dirs`, `sizes`) take any key.
fn unknown_keys(table: &toml::Table) -> Vec<String> {
    const TOP: &[&str] = &[
        "theme",
        "window",
        "panels",
        "sections",
        "visibility",
        "colour",
        "recent",
        "export_dirs",
        "tab",
        "library_units",
        "sizes",
        "snap",
        "drawing",
        "text",
    ];
    const WINDOW: &[&str] = &["x", "y", "width", "height", "maximized"];
    const PANELS: &[&str] = &[
        "left_width",
        "left_open",
        "right_width",
        "right_open",
        "warnings_open",
    ];
    const VISIBILITY: &[&str] = &[
        "cuts",
        "rapids",
        "plunges",
        "arrows",
        "start_points",
        "tab_marks",
        "grid",
        "extents",
        "only_selected",
    ];
    const TAB: &[&str] = &["width_mm", "height_mm", "shape"];
    const SNAP: &[&str] = &["enabled", "kinds", "radius_px", "axis"];
    const DRAWING: &[&str] = &[
        "tool",
        "grid_spacing_mm",
        "corners",
        "corner_mm",
        "polygon_sides",
        "polygon_measured",
    ];
    const TEXT: &[&str] = &[
        "family",
        "style",
        "height_mm",
        "letter_spacing",
        "line_spacing",
        "align",
    ];
    let mut unknown = Vec::new();
    for (key, value) in table {
        if !TOP.contains(&key.as_str()) {
            unknown.push(key.clone());
            continue;
        }
        let known = match key.as_str() {
            "window" => WINDOW,
            "panels" => PANELS,
            "visibility" => VISIBILITY,
            "tab" => TAB,
            "snap" => SNAP,
            "drawing" => DRAWING,
            "text" => TEXT,
            _ => continue,
        };
        if let toml::Value::Table(inner) = value {
            for inner_key in inner.keys() {
                if !known.contains(&inner_key.as_str()) {
                    unknown.push(format!("{key}.{inner_key}"));
                }
            }
        }
    }
    unknown
}

/// The 1-based line and column of the last segment of dotted key `key` in
/// `text`, where a line starts with it; `None` when it is written some
/// other way (inline, or quoted).
fn key_position(text: &str, key: &str) -> Option<(usize, usize)> {
    let last = key.rsplit('.').next()?;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed
            .strip_prefix(last)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
        {
            return Some(line_column(text, offset + (line.len() - trimmed.len())));
        }
        offset += line.len();
    }
    None
}

/// Writes `prefs` as `gui.toml` of `paths`, creating the config directory
/// if needed, through a staged write so a crash leaves the old file.
///
/// # Errors
///
/// [`ShellError::Create`] when the directory cannot be made, and the
/// staged write's errors.
pub fn save(paths: &Paths, prefs: &GuiPrefs) -> Result<(), ShellError> {
    let text = to_toml(prefs);
    std::fs::create_dir_all(&paths.dir).map_err(|source| ShellError::Create {
        path: paths.dir.clone(),
        source,
    })?;
    StagedFile::write(&paths.gui_prefs_file, text.as_bytes())?.commit()
}

/// `prefs` as the file's text.
#[must_use]
pub fn to_toml(prefs: &GuiPrefs) -> String {
    // A record of plain values and maps of plain values cannot fail to
    // serialize; an empty string would mean it somehow did, and `parse`
    // reads that back as the defaults.
    toml::to_string_pretty(prefs).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory for test `name`.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dc-gui-prefs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Design "Preferences": a missing `gui.toml` is the defaults, with
    /// nothing to report.
    #[test]
    fn missing_file_is_defaults() {
        let dir = scratch("missing");
        let paths = Paths::in_dir(dir.join("nowhere"));
        let loaded = load(&paths);
        assert_eq!(loaded.prefs, GuiPrefs::default());
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert_eq!(loaded.prefs.theme, Theme::Dark);
        assert!(loaded.prefs.visibility.cuts && !loaded.prefs.visibility.arrows);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Every field survives a save and a load, through the file.
    #[test]
    fn round_trip() {
        let dir = scratch("round-trip");
        let paths = Paths::in_dir(dir.join("config"));
        let prefs = GuiPrefs {
            theme: Theme::Dark,
            window: Some(WindowGeometry {
                x: 10.0,
                y: 20.0,
                width: 1440.0,
                height: 900.0,
                maximized: true,
            }),
            panels: Panels {
                left_width: 300.0,
                left_open: false,
                right_width: 360.0,
                right_open: true,
                warnings_open: true,
            },
            sections: [("project.stock".to_owned(), false)].into_iter().collect(),
            visibility: Visibility {
                arrows: true,
                grid: false,
                ..Visibility::default()
            },
            colour: ColourMode::ByDepth,
            recent: vec![PathBuf::from("/jobs/coaster.dcv")],
            export_dirs: [("/jobs/coaster.dcv".to_owned(), PathBuf::from("/jobs/out"))]
                .into_iter()
                .collect(),
            tab: TabSize {
                width_mm: 10.0,
                height_mm: 2.5,
                shape: TabShape::Smooth,
            },
            library_units: Some(LibraryUnits::In),
            sizes: [("library.width".to_owned(), 1240.0)].into_iter().collect(),
            snap: Snap {
                enabled: false,
                kinds: vec!["endpoint".to_owned(), "perpendicular".to_owned()],
                radius_px: 20.0,
                axis: false,
            },
            drawing: Drawing {
                tool: "slot".to_owned(),
                grid_spacing_mm: Some(6.35),
                corners: "rounded".to_owned(),
                corner_mm: Some(3.0),
                polygon_sides: 8,
                polygon_measured: "flats".to_owned(),
            },
            text: Text {
                family: "Source Serif 4".to_owned(),
                style: "Bold".to_owned(),
                height_mm: Some(12.5),
                letter_spacing: 0.1,
                line_spacing: 1.2,
                align: "centre".to_owned(),
            },
        };
        save(&paths, &prefs).unwrap();
        let loaded = load(&paths);
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert_eq!(loaded.prefs, prefs);
        // And the text itself reads back to the same record.
        assert_eq!(parse(&to_toml(&prefs), &paths.gui_prefs_file).prefs, prefs);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Phase 5: a file from before the drawing tools has no `[snap]` and
    /// no `[drawing]`, and reads as the defaults for both: every kind but
    /// perpendicular at 14 px, the Rectangle tool, a grid that follows
    /// the zoom. An unpinned grid is written as no key at all, and an
    /// unknown key inside either table is noted like any other.
    #[test]
    fn snap_and_drawing_default_when_missing() {
        let path = Path::new("/somewhere/gui.toml");
        let loaded = parse("theme = \"light\"\n", path);
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert!(loaded.prefs.snap.enabled);
        assert_eq!(loaded.prefs.snap.kinds.len(), 6);
        assert!(
            !loaded
                .prefs
                .snap
                .kinds
                .contains(&"perpendicular".to_owned())
        );
        assert!((loaded.prefs.snap.radius_px - 14.0).abs() < 1e-9);
        assert_eq!(loaded.prefs.drawing.tool, "rectangle");
        assert_eq!(loaded.prefs.drawing.grid_spacing_mm, None);
        let text = to_toml(&GuiPrefs::default());
        assert!(text.contains("[snap]"), "{text}");
        assert!(!text.contains("grid_spacing_mm"), "{text}");
        let loaded = parse(
            "[snap]\nenabled = false\nradius = 3\n\n[drawing]\ntool = \"gear\"\nsides = 6\n",
            path,
        );
        assert!(!loaded.prefs.snap.enabled);
        assert_eq!(loaded.prefs.drawing.tool, "gear");
        assert_eq!(loaded.notes.len(), 2, "{:?}", loaded.notes);
        for key in ["unknown key `snap.radius`", "unknown key `drawing.sides`"] {
            assert!(
                loaded.notes.iter().any(|note| note.contains(key)),
                "{key}: {:?}",
                loaded.notes
            );
        }
    }

    /// Design "Preferences": a key this build does not know (another
    /// DreamCarve's) is a note naming it and its line, not an error; the
    /// rest of the file still applies. So is a file that does not parse.
    #[test]
    fn unknown_field_is_a_note_not_an_error() {
        let path = Path::new("/somewhere/gui.toml");
        let loaded = parse(
            "theme = \"light\"\nfuture_key = 1\n\n[panels]\nleft_width = 250.0\nleft_width_px = 3\n",
            path,
        );
        assert_eq!(loaded.prefs.theme, Theme::Light);
        assert!((loaded.prefs.panels.left_width - 250.0).abs() < 1e-9);
        assert_eq!(loaded.notes.len(), 2, "{:?}", loaded.notes);
        assert_eq!(loaded.unread, None, "the file was read: its values apply");
        assert!(
            loaded.notes[0].starts_with("/somewhere/gui.toml:2:1: unknown key `future_key`"),
            "{}",
            loaded.notes[0]
        );
        assert!(
            loaded.notes[1]
                .starts_with("/somewhere/gui.toml:6:1: unknown key `panels.left_width_px`"),
            "{}",
            loaded.notes[1]
        );
        // A syntax error: the defaults, and a note at the line.
        let loaded = parse("theme = light\n", path);
        assert_eq!(loaded.prefs, GuiPrefs::default());
        assert_eq!(loaded.notes.len(), 1, "{:?}", loaded.notes);
        assert!(
            loaded.notes[0].starts_with("/somewhere/gui.toml:1:"),
            "{}",
            loaded.notes[0]
        );
        assert_eq!(
            loaded.unread.as_ref(),
            loaded.notes.first(),
            "it did not parse: the caller is told, with the file and the line"
        );
        // A value of the wrong kind: the defaults, with the reason.
        let loaded = parse("theme = \"sepia\"\n", path);
        assert_eq!(loaded.prefs, GuiPrefs::default());
        assert_eq!(loaded.notes.len(), 1, "{:?}", loaded.notes);
        assert!(loaded.notes[0].contains("sepia"), "{}", loaded.notes[0]);
        assert_eq!(loaded.unread.as_ref(), loaded.notes.first());
    }

    /// Slice D9: `load` tells a file that is there and could not be used
    /// (`unread`, so the app keeps its hands off it) from one that is
    /// missing, which is the defaults with nothing to say, and from one
    /// that was read.
    #[test]
    fn a_file_that_could_not_be_used_is_reported_as_unread() {
        let dir = scratch("unread");
        let paths = Paths::in_dir(dir.join("config"));
        assert_eq!(load(&paths).unread, None, "missing");
        std::fs::create_dir_all(&paths.dir).unwrap();
        std::fs::write(&paths.gui_prefs_file, "theme = \"light\"\n").unwrap();
        let read = load(&paths);
        assert_eq!((read.unread, read.prefs.theme), (None, Theme::Light));
        std::fs::write(&paths.gui_prefs_file, "theme = \"light\"\n[panels\n").unwrap();
        let damaged = load(&paths);
        assert_eq!(damaged.prefs, GuiPrefs::default());
        let why = damaged.unread.expect("a damaged file is unread");
        assert!(
            why.starts_with(&format!("{}:2:", paths.gui_prefs_file.display())),
            "it names the file and the line: {why}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
