// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shell's files (`docs/specs/phase-4/design.md`, "Architecture → The
//! shell", "Files"; functional spec, capability 51): reading and writing a
//! `.dcv` (the write staged, as the CLI's), the Recent list, `gui.toml`
//! read into `dc-app`'s preferences and written back, and the kind of a
//! dropped or chosen file. A drawing's bytes go to the Import drawing
//! sheet (`panels::import`), which parses them with `dc-import`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dc_app::{Prefs, Theme};
use dc_import::{DxfImportOptions, SvgImportOptions};
use dc_job::Drawing;
use dreamcarve_shell::gui_prefs;
use dreamcarve_shell::{GuiPrefs, ShellError, StagedFile};

/// The Recent list as the File menu shows it: the files of
/// `prefs.recent` that still exist, newest first. Files that have gone
/// (deleted, moved, an unplugged drive) are dropped from `prefs` too, so
/// the list never offers a file that cannot open.
pub fn recent(prefs: &mut Prefs) -> Vec<PathBuf> {
    prefs.recent.retain(|path| path.is_file());
    prefs.recent.clone()
}

/// Whether any file of the Recent list has gone.
#[must_use]
pub fn recent_has_missing(prefs: &Prefs) -> bool {
    prefs.recent.iter().any(|path| !path.is_file())
}

/// The bytes of the project at `path`.
///
/// # Errors
///
/// The OS's, when the file cannot be read.
pub fn read(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Writes `bytes` to `path` through a staged write: a crash or a full disk
/// leaves the old file whole.
///
/// # Errors
///
/// The staged write's.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), ShellError> {
    StagedFile::write(path, bytes)?.commit()
}

/// `path`'s file name, for titles, menus and messages.
#[must_use]
pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// `path`'s file name without its extension: `plaque` for
/// `plaque.dcv`, the window title's name (capability 50).
pub fn stem(path: &Path) -> String {
    path.file_stem().map_or_else(
        || file_name(path),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

/// What a dropped or chosen file is, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A DreamCarve project.
    Project,
    /// An SVG drawing.
    Svg,
    /// A DXF drawing.
    Dxf,
    /// A PNG or JPEG picture (capability 56), which waits for phase 3.4.
    Picture,
}

/// The kind of file at `path`; `None` for anything else.
#[must_use]
pub fn kind(path: &Path) -> Option<FileKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "dcv" => Some(FileKind::Project),
        "svg" => Some(FileKind::Svg),
        "dxf" => Some(FileKind::Dxf),
        "png" | "jpg" | "jpeg" => Some(FileKind::Picture),
        _ => None,
    }
}

/// The drawing in `bytes`, read as `kind` with the importer's default
/// options, for the bundled coaster example; a user's drawing goes
/// through the Import drawing sheet.
///
/// # Errors
///
/// The importer's message, for the status bar; also when `kind` is not a
/// drawing.
pub fn read_drawing(kind: FileKind, bytes: &[u8]) -> Result<Drawing, String> {
    match kind {
        FileKind::Svg => dc_import::import_svg(bytes, &SvgImportOptions::default())
            .map(|(import, _warnings)| Drawing::Svg(import))
            .map_err(|e| e.to_string()),
        FileKind::Dxf => dc_import::import_dxf(bytes, &DxfImportOptions::default())
            .map(|(import, _warnings)| Drawing::Dxf(import))
            .map_err(|e| e.to_string()),
        FileKind::Project | FileKind::Picture => Err(String::new()),
    }
}

/// `gui.toml`'s record as `dc-app`'s preferences.
#[must_use]
pub fn prefs_from_file(file: &GuiPrefs) -> Prefs {
    Prefs {
        window: file.window.map(|w| dc_app::WindowGeometry {
            x: w.x,
            y: w.y,
            width: w.width,
            height: w.height,
            maximized: w.maximized,
        }),
        panels: dc_app::Panels {
            left_width: file.panels.left_width,
            left_open: file.panels.left_open,
            right_width: file.panels.right_width,
            right_open: file.panels.right_open,
            warnings_open: file.panels.warnings_open,
        },
        sections: file.sections.clone(),
        visibility: dc_app::Visibility {
            cuts: file.visibility.cuts,
            rapids: file.visibility.rapids,
            plunges: file.visibility.plunges,
            arrows: file.visibility.arrows,
            start_points: file.visibility.start_points,
            tab_marks: file.visibility.tab_marks,
            grid: file.visibility.grid,
            extents: file.visibility.extents,
            only_selected: file.visibility.only_selected,
        },
        colour: match file.colour {
            gui_prefs::ColourMode::ByOperation => dc_app::ColourMode::ByOperation,
            gui_prefs::ColourMode::ByDepth => dc_app::ColourMode::ByDepth,
        },
        recent: file
            .recent
            .iter()
            .take(Prefs::MAX_RECENT)
            .cloned()
            .collect(),
        export_dirs: file
            .export_dirs
            .iter()
            .map(|(project, dir)| (PathBuf::from(project), dir.clone()))
            .collect(),
        theme: match file.theme {
            gui_prefs::Theme::System => Theme::System,
            gui_prefs::Theme::Light => Theme::Light,
            gui_prefs::Theme::Dark => Theme::Dark,
        },
        tab: dc_app::prefs::TabSize {
            width_mm: file.tab.width_mm,
            height_mm: file.tab.height_mm,
            shape: match file.tab.shape {
                gui_prefs::TabShape::Rect => dc_project::TabShape::Rect,
                gui_prefs::TabShape::Smooth => dc_project::TabShape::Smooth,
            },
        },
    }
}

/// `dc-app`'s preferences as `gui.toml`'s record.
#[must_use]
pub fn prefs_to_file(prefs: &Prefs) -> GuiPrefs {
    GuiPrefs {
        theme: match prefs.theme {
            Theme::System => gui_prefs::Theme::System,
            Theme::Light => gui_prefs::Theme::Light,
            Theme::Dark => gui_prefs::Theme::Dark,
        },
        window: prefs.window.map(|w| gui_prefs::WindowGeometry {
            x: w.x,
            y: w.y,
            width: w.width,
            height: w.height,
            maximized: w.maximized,
        }),
        panels: gui_prefs::Panels {
            left_width: prefs.panels.left_width,
            left_open: prefs.panels.left_open,
            right_width: prefs.panels.right_width,
            right_open: prefs.panels.right_open,
            warnings_open: prefs.panels.warnings_open,
        },
        sections: prefs.sections.clone(),
        visibility: gui_prefs::Visibility {
            cuts: prefs.visibility.cuts,
            rapids: prefs.visibility.rapids,
            plunges: prefs.visibility.plunges,
            arrows: prefs.visibility.arrows,
            start_points: prefs.visibility.start_points,
            tab_marks: prefs.visibility.tab_marks,
            grid: prefs.visibility.grid,
            extents: prefs.visibility.extents,
            only_selected: prefs.visibility.only_selected,
        },
        colour: match prefs.colour {
            dc_app::ColourMode::ByOperation => gui_prefs::ColourMode::ByOperation,
            dc_app::ColourMode::ByDepth => gui_prefs::ColourMode::ByDepth,
        },
        recent: prefs.recent.clone(),
        export_dirs: prefs
            .export_dirs
            .iter()
            .map(|(project, dir)| (project.display().to_string(), dir.clone()))
            .collect::<BTreeMap<_, _>>(),
        tab: gui_prefs::TabSize {
            width_mm: prefs.tab.width_mm,
            height_mm: prefs.tab.height_mm,
            shape: match prefs.tab.shape {
                dc_project::TabShape::Rect => gui_prefs::TabShape::Rect,
                dc_project::TabShape::Smooth => gui_prefs::TabShape::Smooth,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every preference survives the trip to `gui.toml`'s record and back.
    #[test]
    fn prefs_round_trip_through_the_file_record() {
        let mut prefs = Prefs {
            theme: Theme::Dark,
            window: Some(dc_app::WindowGeometry {
                x: 5.0,
                y: 6.0,
                width: 1400.0,
                height: 900.0,
                maximized: true,
            }),
            colour: dc_app::ColourMode::ByDepth,
            ..Prefs::default()
        };
        prefs.panels.left_open = false;
        prefs.panels.right_width = 333.0;
        prefs.visibility.arrows = true;
        prefs.tab.width_mm = 12.0;
        prefs.tab.shape = dc_project::TabShape::Smooth;
        prefs.sections.insert("project.stock".to_owned(), false);
        prefs.recent = vec![PathBuf::from("/a.dcv"), PathBuf::from("/b.dcv")];
        prefs
            .export_dirs
            .insert(PathBuf::from("/a.dcv"), PathBuf::from("/out"));
        assert_eq!(prefs_from_file(&prefs_to_file(&prefs)), prefs);
        assert_eq!(prefs_from_file(&GuiPrefs::default()), Prefs::default());
    }

    #[test]
    fn kinds_follow_the_extension_in_any_case() {
        assert_eq!(kind(Path::new("a/b.DCV")), Some(FileKind::Project));
        assert_eq!(kind(Path::new("plaque.svg")), Some(FileKind::Svg));
        assert_eq!(kind(Path::new("plaque.Dxf")), Some(FileKind::Dxf));
        assert_eq!(kind(Path::new("photo.png")), Some(FileKind::Picture));
        assert_eq!(kind(Path::new("photo.JPEG")), Some(FileKind::Picture));
        assert_eq!(kind(Path::new("notes.txt")), None);
        assert_eq!(kind(Path::new("README")), None);
    }
}
