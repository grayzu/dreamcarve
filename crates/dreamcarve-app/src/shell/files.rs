// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shell's files (`docs/specs/phase-4/design.md`, "Architecture → The
//! shell", "Files"; functional spec, capability 51): reading and writing a
//! `.dcv` (the write staged, as the CLI's), the Recent list, `gui.toml`
//! read into `dc-app`'s preferences and written back. What a dropped or
//! chosen file is, by its name, is `dc_app::import::FileKind`. A drawing's
//! bytes go to the Import drawing sheet (`panels::import`), which parses
//! them with `dc-import`; a picture's to the Add picture sheet
//! (`panels::picture`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dc_app::import::DrawingFormat;
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

/// The bytes of the file at `path`: a project, a drawing, a picture or a
/// tool library, read under the size limit the CLI reads under
/// ([`dreamcarve_shell::MAX_FILE_BYTES`]; review 1.6).
///
/// # Errors
///
/// Why not, for a message that already names the file: the OS's reason
/// when the file cannot be read, as before; for a file past the limit,
/// the shell's whole message, with its size and the limit.
pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    dreamcarve_shell::read_capped(path, dreamcarve_shell::MAX_FILE_BYTES).map_err(|e| match e {
        ShellError::Read { source, .. } => source.to_string(),
        other => other.to_string(),
    })
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

/// The drawing in `bytes`, read as `format` with the importer's default
/// options, for the bundled coaster example; a user's drawing goes
/// through the Import drawing sheet.
///
/// # Errors
///
/// The importer's message, for the status bar.
pub fn read_drawing(format: DrawingFormat, bytes: &[u8]) -> Result<Drawing, String> {
    match format {
        DrawingFormat::Svg => dc_import::import_svg(bytes, &SvgImportOptions::default())
            .map(|(import, _warnings)| Drawing::Svg(import))
            .map_err(|e| e.to_string()),
        DrawingFormat::Dxf => dc_import::import_dxf(bytes, &DxfImportOptions::default())
            .map(|(import, _warnings)| Drawing::Dxf(import))
            .map_err(|e| e.to_string()),
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
        library_units: file.library_units.map(|units| match units {
            gui_prefs::LibraryUnits::Mm => dc_project::Units::Mm,
            gui_prefs::LibraryUnits::In => dc_project::Units::Inch,
        }),
        sizes: file.sizes.clone(),
        snap: dc_app::SnapSettings {
            enabled: file.snap.enabled,
            kinds: file
                .snap
                .kinds
                .iter()
                .filter_map(|kind| dc_app::SnapKind::from_key(kind))
                .collect(),
            radius_px: file.snap.radius_px,
        }
        .normalized(),
        grid_spacing_mm: file
            .drawing
            .grid_spacing_mm
            .filter(|spacing| spacing.is_finite() && *spacing > 0.0),
        drawing_tool: dc_app::Drawing::from_key(&file.drawing.tool).unwrap_or_default(),
        drawing: dc_app::prefs::DrawingPrefs {
            corners: dc_app::tool::CornerStyle::from_key(&file.drawing.corners).unwrap_or_default(),
            corner_mm: file
                .drawing
                .corner_mm
                .filter(|size| size.is_finite() && *size > 0.0),
            polygon_sides: file
                .drawing
                .polygon_sides
                .clamp(dc_project::MIN_POLYGON_SIDES, dc_project::MAX_POLYGON_SIDES),
            polygon_measured: match file.drawing.polygon_measured.as_str() {
                "flats" => dc_project::Measured::Flats,
                _ => dc_project::Measured::Corners,
            },
        },
        // Session state: what Offset…, Fillet Corners… and Join were last
        // given is not in `gui.toml`.
        adjust: dc_app::tool::AdjustPrefs::default(),
        text: dc_app::prefs::TextPrefs {
            font: dc_project::FontRef {
                family: file.text.family.clone(),
                style: file.text.style.clone(),
            },
            height_mm: file
                .text
                .height_mm
                .filter(|height| height.is_finite() && *height > 0.0),
            letter_spacing_fraction: Some(file.text.letter_spacing)
                .filter(|spacing| spacing.is_finite())
                .unwrap_or(0.0),
            line_spacing_fraction: Some(file.text.line_spacing)
                .filter(|spacing| spacing.is_finite() && *spacing > 0.0)
                .unwrap_or(1.0),
            align: match file.text.align.as_str() {
                "centre" => dc_project::Align::Centre,
                "right" => dc_project::Align::Right,
                _ => dc_project::Align::Left,
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
        library_units: prefs.library_units.map(|units| match units {
            dc_project::Units::Mm => gui_prefs::LibraryUnits::Mm,
            dc_project::Units::Inch => gui_prefs::LibraryUnits::In,
        }),
        sizes: prefs.sizes.clone(),
        snap: gui_prefs::Snap {
            enabled: prefs.snap.enabled,
            kinds: prefs
                .snap
                .kinds
                .iter()
                .map(|kind| kind.key().to_owned())
                .collect(),
            radius_px: prefs.snap.radius_px,
        },
        drawing: gui_prefs::Drawing {
            tool: prefs.drawing_tool.key().to_owned(),
            grid_spacing_mm: prefs.grid_spacing_mm,
            corners: prefs.drawing.corners.key().to_owned(),
            corner_mm: prefs.drawing.corner_mm,
            polygon_sides: prefs.drawing.polygon_sides,
            polygon_measured: match prefs.drawing.polygon_measured {
                dc_project::Measured::Corners => "corners",
                dc_project::Measured::Flats => "flats",
            }
            .to_owned(),
        },
        text: gui_prefs::Text {
            family: prefs.text.font.family.clone(),
            style: prefs.text.font.style.clone(),
            height_mm: prefs.text.height_mm,
            letter_spacing: prefs.text.letter_spacing_fraction,
            line_spacing: prefs.text.line_spacing_fraction,
            align: match prefs.text.align {
                dc_project::Align::Left => "left",
                dc_project::Align::Centre => "centre",
                dc_project::Align::Right => "right",
            }
            .to_owned(),
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
        prefs.library_units = Some(dc_project::Units::Inch);
        prefs.sizes.insert("library.width".to_owned(), 1240.0);
        prefs.sections.insert("project.stock".to_owned(), false);
        prefs.recent = vec![PathBuf::from("/a.dcv"), PathBuf::from("/b.dcv")];
        prefs
            .export_dirs
            .insert(PathBuf::from("/a.dcv"), PathBuf::from("/out"));
        prefs.snap.enabled = false;
        prefs.snap.set(dc_app::SnapKind::Perpendicular, true);
        prefs.snap.set(dc_app::SnapKind::Grid, false);
        prefs.snap.radius_px = 20.0;
        prefs.grid_spacing_mm = Some(6.35);
        prefs.drawing_tool = dc_app::Drawing::Slot;
        prefs.drawing = dc_app::prefs::DrawingPrefs {
            corners: dc_app::tool::CornerStyle::Chamfered,
            corner_mm: Some(6.35),
            polygon_sides: 8,
            polygon_measured: dc_project::Measured::Flats,
        };
        prefs.text = dc_app::prefs::TextPrefs {
            font: dc_project::FontRef {
                family: "Pacifico".to_owned(),
                style: "Regular".to_owned(),
            },
            height_mm: Some(12.5),
            letter_spacing_fraction: 0.1,
            line_spacing_fraction: 1.5,
            align: dc_project::Align::Right,
        };
        assert_eq!(prefs_from_file(&prefs_to_file(&prefs)), prefs);
        assert_eq!(prefs_from_file(&GuiPrefs::default()), Prefs::default());
    }

    /// What `gui.toml` holds for snapping and drawing is held to what the
    /// app can use: a snap kind or a tool this build does not know is
    /// left out, a radius outside 6 to 32 px is held to it, and a grid
    /// spacing that is no length is no pin.
    #[test]
    fn snap_and_drawing_from_a_file_are_held_to_what_the_app_can_use() {
        let mut file = GuiPrefs::default();
        file.snap.kinds = vec!["endpoint".to_owned(), "tangent".to_owned()];
        file.snap.radius_px = 500.0;
        file.drawing.tool = "pen".to_owned();
        file.drawing.grid_spacing_mm = Some(-1.0);
        file.drawing.corners = "bevelled".to_owned();
        file.drawing.corner_mm = Some(0.0);
        file.drawing.polygon_sides = 900;
        file.drawing.polygon_measured = "points".to_owned();
        let prefs = prefs_from_file(&file);
        assert_eq!(
            prefs.drawing,
            dc_app::prefs::DrawingPrefs {
                polygon_sides: 50,
                ..dc_app::prefs::DrawingPrefs::default()
            }
        );
        assert_eq!(
            prefs.snap.kinds.iter().copied().collect::<Vec<_>>(),
            [dc_app::SnapKind::Endpoint]
        );
        assert!((prefs.snap.radius_px - 32.0).abs() < 1e-9);
        assert_eq!(prefs.drawing_tool, dc_app::Drawing::Rectangle);
        assert_eq!(prefs.grid_spacing_mm, None);
    }

    #[test]
    fn kinds_follow_the_extension_in_any_case() {
        use dc_app::import::FileKind;
        use dc_import::ImageFormat;

        // What the window asks of a dropped or chosen file: its name, as
        // `file_name` gives it, read by `dc_app::import::FileKind`.
        let kind = |path: &str| FileKind::of(&file_name(Path::new(path)));
        assert_eq!(kind("a/b.DCV"), Some(FileKind::Project));
        assert_eq!(
            kind("plaque.svg"),
            Some(FileKind::Drawing(DrawingFormat::Svg))
        );
        assert_eq!(
            kind("plaque.Dxf"),
            Some(FileKind::Drawing(DrawingFormat::Dxf))
        );
        assert_eq!(kind("photo.png"), Some(FileKind::Picture(ImageFormat::Png)));
        assert_eq!(
            kind("photo.JPEG"),
            Some(FileKind::Picture(ImageFormat::Jpeg))
        );
        assert_eq!(kind("notes.txt"), None);
        assert_eq!(kind("README"), None);
    }
}
