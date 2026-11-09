// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The app's preferences file, `<config dir>/gui.toml`
//! (`docs/specs/phase-4/design.md`, "Preferences"; functional spec, open
//! question 11): the window and panel layout, the toolpath display
//! settings, the recent files, where each project last exported, and the
//! theme. View state, never project state: nothing here changes a program
//! or reaches the `.dcv`.
//!
//! Read at start and written on change and on exit. A missing file is the
//! defaults. Unlike `config.toml`, a file that does not parse, or has a key
//! this build does not know (an older or newer DreamCarve wrote it), is
//! the defaults plus a one-line note for the Warnings panel, never a
//! refusal to start: a preference is not worth losing the session over.

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
    /// Light or dark, or the system's choice.
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

/// The app's appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
            left_width: 280.0,
            left_open: true,
            right_width: 320.0,
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
}

/// Reads `gui.toml` of `paths`. A missing file is the defaults with no
/// note; a file that cannot be read or parsed is the defaults with a note
/// saying why.
#[must_use]
pub fn load(paths: &Paths) -> Loaded {
    let path = &paths.gui_prefs_file;
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded::default(),
        Err(e) => Loaded {
            prefs: GuiPrefs::default(),
            notes: vec![format!("cannot read {}: {e}", path.display())],
        },
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
        Err(e) => {
            notes.push(describe_error(text, path, &e));
            return Loaded {
                prefs: GuiPrefs::default(),
                notes,
            };
        }
    };
    for key in unknown_keys(&table) {
        let at = key_position(text, &key)
            .map_or_else(String::new, |(line, column)| format!(":{line}:{column}"));
        notes.push(format!(
            "{}{at}: unknown key `{key}` ignored; a different DreamCarve wrote it",
            path.display()
        ));
    }
    let prefs = match table.try_into::<GuiPrefs>() {
        Ok(prefs) => prefs,
        Err(e) => {
            notes.push(describe_error(text, path, &e));
            GuiPrefs::default()
        }
    };
    Loaded { prefs, notes }
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
/// `export_dirs`) take any key.
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
        assert_eq!(loaded.prefs.theme, Theme::System);
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
        };
        save(&paths, &prefs).unwrap();
        let loaded = load(&paths);
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
        assert_eq!(loaded.prefs, prefs);
        // And the text itself reads back to the same record.
        assert_eq!(parse(&to_toml(&prefs), &paths.gui_prefs_file).prefs, prefs);
        std::fs::remove_dir_all(&dir).unwrap();
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
        // A value of the wrong kind: the defaults, with the reason.
        let loaded = parse("theme = \"sepia\"\n", path);
        assert_eq!(loaded.prefs, GuiPrefs::default());
        assert_eq!(loaded.notes.len(), 1, "{:?}", loaded.notes);
        assert!(loaded.notes[0].contains("sepia"), "{}", loaded.notes[0]);
    }
}
