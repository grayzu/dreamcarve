// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Import drawing sheet's model (functional spec, capabilities 51 and
//! 55; mock-up `02-import-dialog.svg`; `docs/specs/phase-4/design.md`,
//! "Test plan → Import"). The shell reads the file and hands the bytes
//! here; `dc-import` parses them with the options the sheet shows, which
//! are `dreamcarve init`'s flags: for a DXF the units (`--units`, required
//! when the file declares none), the join tolerance (`--join-tolerance`)
//! and the layers (`--layer`). An SVG has no options. Every change of an
//! option reads the bytes again, so what the sheet shows (the counts, the
//! layer list, the preview, the notes) is what *Import* adds.
//!
//! The sheet imports into the open project ([`Purpose::Import`], through
//! [`crate::App::import_drawing`]) or makes a new project from the drawing
//! ([`Purpose::New`], [`crate::App::new_from_drawing`]).
//!
//! The Add picture sheet's model is [`PictureImport`] (capabilities 51 and
//! 56): the picture decoded once to the grayscale a bitmap stores
//! (`dc_import::bitmap`, as `dreamcarve image add` decodes it), and the one
//! thing the sheet asks, the width *or* the height, with the other
//! following the pixels. It adds a bitmap to the open project
//! ([`crate::App::import_picture`]) or makes a new project round the
//! picture ([`crate::App::new_from_picture`]).
//!
//! [`FileKind::of`] says what a file is by its name, and
//! [`crate::App::drop_file`] what a file dropped on the window becomes
//! ([`Dropped`]): the project opened, or the sheet for its kind.

use std::collections::BTreeSet;
use std::sync::Arc;

use dc_geometry::{Point, Polyline, Rect};
use dc_import::{
    DxfImportOptions, DxfLayer, DxfUnits, ImageFormat, ImportError, ImportWarning,
    JOIN_TOLERANCE_DEFAULT_MM, SvgImportOptions,
};
use dc_job::picture::PictureSize;
use dc_job::{Drawing, JobError};
use dc_project::{Grayscale, Units};

use crate::app::Opened;
use crate::command::Command;
use crate::strings::{StrKey, Strings};

/// What a file is, by its name's extension in any case: what the window
/// does with one dropped on it, passed on the command line or chosen
/// (capabilities 51, 55 and 56).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A `.dcv` project: it opens.
    Project,
    /// An `.svg` or `.dxf` drawing: the Add drawing sheet.
    Drawing(DrawingFormat),
    /// A `.png`, `.jpg` or `.jpeg` picture: the Add picture sheet.
    Picture(ImageFormat),
}

impl FileKind {
    /// The kind of the file named `name`, or `None` when its extension
    /// names nothing DreamCarve reads.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        // `.svg` alone is a hidden file with no extension, as a path
        // reads it.
        let (_, extension) = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty())?;
        if extension.eq_ignore_ascii_case("dcv") {
            Some(Self::Project)
        } else if extension.eq_ignore_ascii_case("svg") {
            Some(Self::Drawing(DrawingFormat::Svg))
        } else if extension.eq_ignore_ascii_case("dxf") {
            Some(Self::Drawing(DrawingFormat::Dxf))
        } else {
            ImageFormat::from_extension(extension).map(Self::Picture)
        }
    }
}

/// What a file dropped on the window became ([`crate::App::drop_file`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Dropped {
    /// A `.dcv`: the project is open, and this is what opening reported.
    Project(Opened),
    /// A drawing: the Add drawing sheet's model, for the window to show.
    Drawing(Box<DrawingImport>),
    /// A picture: the Add picture sheet's model, for the window to show.
    Picture(Box<PictureImport>),
}

/// `name` without its last extension: a new project's name and a bitmap's,
/// as `init` and `image add` name them from the file.
pub(crate) fn stem(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// The drawing formats the sheet reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawingFormat {
    /// An SVG, from Inkscape or a browser: no options.
    Svg,
    /// A DXF, from CAD: units, join tolerance, layers.
    Dxf,
}

/// What the sheet's *Import* does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Adds the shapes to the open project, placed as drawn (File ▸
    /// Import…, a drop).
    Import,
    /// Makes a new project round the drawing, as `init` does without its
    /// placeholder tool and Profile (File ▸ New from drawing…).
    New,
}

/// The counts the sheet's header shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Shapes with a closed outline, which Profile and Pocket can cut.
    pub closed: usize,
    /// Shapes that are open paths only.
    pub open: usize,
    /// Layers in the file (a DXF's); 0 for an SVG.
    pub layers: usize,
}

/// What the sheet's preview draws (capability 55): the drawing's outlines
/// as drawn and their bounds, and nothing else, so no stock outline.
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    /// The shapes' outlines, mm, Y-up, the drawing's origin at 0,0.
    pub outlines: Vec<Polyline>,
    /// Their bounds, mm: the drawing's.
    pub bounds: Rect,
}

/// The sheet's state: the file, the options, and the last read.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingImport {
    format: DrawingFormat,
    name: String,
    bytes: Vec<u8>,
    purpose: Purpose,
    /// The units chosen in the dropdown; `None` reads the file's.
    units: Option<Units>,
    join_tolerance_mm: f64,
    /// The layers unticked. A layer starts ticked, as `init` without
    /// `--layer` imports every layer.
    unticked: BTreeSet<String>,
    /// Whether the file declares no units DreamCarve scales from, as found
    /// by the first read.
    units_required: bool,
    /// The units `$INSUNITS` declares, when it declares usable ones.
    declared: Option<DxfUnits>,
    /// The file's layers, from the last read that worked.
    layers: Vec<DxfLayer>,
    /// The last read: the drawing and its warnings, or the importer's
    /// refusal.
    read: Result<(Drawing, Vec<ImportWarning>), String>,
}

impl DrawingImport {
    /// The sheet over `bytes`, the file `name` (with its extension), read
    /// at once with the importer's defaults: the file's units, the 0.01 mm
    /// join tolerance, every layer.
    #[must_use]
    pub fn open(format: DrawingFormat, name: &str, bytes: Vec<u8>, purpose: Purpose) -> Self {
        let mut sheet = Self {
            format,
            name: name.to_owned(),
            bytes,
            purpose,
            units: None,
            join_tolerance_mm: JOIN_TOLERANCE_DEFAULT_MM,
            unticked: BTreeSet::new(),
            units_required: false,
            declared: None,
            layers: Vec::new(),
            read: Err(String::new()),
        };
        let first = sheet.import();
        sheet.units_required = matches!(first, Err(ImportError::UnitsUnknown { .. }));
        sheet.take(first);
        sheet
    }

    /// The file's format.
    #[must_use]
    pub fn format(&self) -> DrawingFormat {
        self.format
    }

    /// The file's name, with its extension, as the undo label names it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The file's name without its extension: a new project's name, as
    /// `init` names it.
    #[must_use]
    pub fn stem(&self) -> &str {
        stem(&self.name)
    }

    /// What *Import* does.
    #[must_use]
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// Whether the file declares no units, so the dropdown must be set
    /// before *Import* (`init --units`).
    #[must_use]
    pub fn units_required(&self) -> bool {
        self.units_required
    }

    /// The units the file declares in `$INSUNITS`, when it declares
    /// usable ones.
    #[must_use]
    pub fn declared_units(&self) -> Option<DxfUnits> {
        self.declared
    }

    /// The units chosen in the dropdown; `None` reads the file's.
    #[must_use]
    pub fn units(&self) -> Option<Units> {
        self.units
    }

    /// Chooses the drawing's units, overriding the file's; `None` reads
    /// the file's again.
    pub fn set_units(&mut self, units: Option<Units>) {
        self.units = units;
        self.reread();
    }

    /// The join tolerance, mm.
    #[must_use]
    pub fn join_tolerance_mm(&self) -> f64 {
        self.join_tolerance_mm
    }

    /// Sets the join tolerance, mm. A negative or non-finite one is the
    /// importer's refusal, shown as [`DrawingImport::blocked`].
    pub fn set_join_tolerance_mm(&mut self, tolerance_mm: f64) {
        self.join_tolerance_mm = tolerance_mm;
        self.reread();
    }

    /// The file's layers, in name order, with what each holds: empty for
    /// an SVG, or until the file can be read. The list does not change
    /// with the ticks.
    #[must_use]
    pub fn layers(&self) -> &[DxfLayer] {
        &self.layers
    }

    /// Whether layer `name` is ticked for import.
    #[must_use]
    pub fn is_ticked(&self, name: &str) -> bool {
        !self.unticked.contains(name)
    }

    /// Ticks or unticks layer `name`.
    pub fn set_layer(&mut self, name: &str, ticked: bool) {
        let changed = if ticked {
            self.unticked.remove(name)
        } else {
            self.unticked.insert(name.to_owned())
        };
        if changed {
            self.reread();
        }
    }

    /// What the last read holds, or `None` when the file could not be
    /// read with these options.
    #[must_use]
    pub fn summary(&self) -> Option<Summary> {
        let (drawing, _) = self.read.as_ref().ok()?;
        let definitions = match drawing {
            Drawing::Svg(import) => &import.definitions,
            Drawing::Dxf(import) => &import.definitions,
        };
        let closed = definitions
            .iter()
            .filter(|d| d.profiles.iter().any(Polyline::is_closed))
            .count();
        Some(Summary {
            closed,
            open: definitions.len() - closed,
            layers: self.layers.len(),
        })
    }

    /// Why *Import* is disabled: the importer's refusal (for a DXF with no
    /// units, the message `init` prints without `--units`), or that the
    /// options leave nothing to import. `None` when it is enabled.
    #[must_use]
    pub fn blocked(&self) -> Option<String> {
        match &self.read {
            Err(message) => Some(message.clone()),
            Ok((drawing, _)) if drawing.bounds().is_none() => Some(JobError::NoShapes.to_string()),
            Ok(_) => None,
        }
    }

    /// The importer's warnings for the last read: what the Warnings panel
    /// lists after the import.
    #[must_use]
    pub fn warnings(&self) -> &[ImportWarning] {
        self.read.as_ref().map_or(&[], |(_, warnings)| warnings)
    }

    /// The drawing *Import* adds, when it is enabled.
    #[must_use]
    pub fn drawing(&self) -> Option<&Drawing> {
        match &self.read {
            Ok((drawing, _)) if drawing.bounds().is_some() => Some(drawing),
            _ => None,
        }
    }

    /// The command *Import* runs into the open project, when it is
    /// enabled: one undo step, *Import plaque.dxf*.
    #[must_use]
    pub fn command(&self) -> Option<Command> {
        Some(Command::ImportDrawing {
            drawing: self.drawing()?.clone(),
            name: self.name.clone(),
        })
    }

    /// The shapes' outlines as drawn (mm, Y-up, the drawing's origin at
    /// 0,0), for the preview: empty when nothing would be imported.
    #[must_use]
    pub fn outlines(&self) -> Vec<Polyline> {
        let Ok((drawing, _)) = &self.read else {
            return Vec::new();
        };
        let (definitions, instances) = match drawing {
            Drawing::Svg(import) => (&import.definitions, &import.instances),
            Drawing::Dxf(import) => (&import.definitions, &import.instances),
        };
        instances
            .iter()
            .filter_map(|instance| {
                let definition = definitions.iter().find(|d| d.id == instance.definition)?;
                Some(
                    definition
                        .profiles
                        .iter()
                        .filter_map(|p| instance.transform.apply_polyline(p).ok()),
                )
            })
            .flatten()
            .collect()
    }

    /// The bounds of [`DrawingImport::outlines`], mm.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        self.read.as_ref().ok()?.0.bounds()
    }

    /// The sheet's preview: the drawing alone, as drawn (capability 55),
    /// with no stock outline and no origin, for the sheet to fit to its
    /// box. `None` when nothing would be imported.
    #[must_use]
    pub fn preview(&self) -> Option<Preview> {
        Some(Preview {
            bounds: self.bounds()?,
            outlines: self.outlines(),
        })
    }

    /// Reads the bytes with the current options and keeps the result.
    fn reread(&mut self) {
        let read = self.import();
        self.take(read);
    }

    fn take(&mut self, read: Result<(Drawing, Vec<ImportWarning>), ImportError>) {
        if let Ok((Drawing::Dxf(import), _)) = &read {
            self.declared = import.units;
            self.layers.clone_from(&import.layers);
        }
        self.read = read.map_err(|e| e.to_string());
    }

    fn import(&self) -> Result<(Drawing, Vec<ImportWarning>), ImportError> {
        match self.format {
            DrawingFormat::Svg => dc_import::import_svg(&self.bytes, &SvgImportOptions::new())
                .map(|(import, warnings)| (Drawing::Svg(import), warnings)),
            DrawingFormat::Dxf => {
                let layers = (!self.unticked.is_empty()).then(|| {
                    self.layers
                        .iter()
                        .map(|l| l.name.clone())
                        .filter(|name| !self.unticked.contains(name))
                        .collect()
                });
                let options = DxfImportOptions {
                    units: self.units,
                    join_tolerance_mm: self.join_tolerance_mm,
                    layers,
                    ..DxfImportOptions::new()
                };
                dc_import::import_dxf(&self.bytes, &options)
                    .map(|(import, warnings)| (Drawing::Dxf(import), warnings))
            }
        }
    }
}

/// Which side of the picture the sheet's size gives; the other follows
/// the pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PictureSide {
    /// The footprint's width (`image add --width`).
    Width,
    /// The footprint's height (`image add --height`).
    Height,
}

/// Why the Add picture sheet's *Import* is disabled.
enum Refusal<'a> {
    /// The file could not be decoded: the importer's message.
    Unreadable(&'a str),
    /// Neither the width nor the height is typed yet.
    SizeMissing,
    /// The size cannot be placed: `image add`'s refusal, or `init`'s.
    Placement(JobError),
}

/// The Add picture sheet's state (capabilities 51 and 56): the picture,
/// decoded once, and the size typed. A picture has no other option.
#[derive(Debug, Clone, PartialEq)]
pub struct PictureImport {
    format: ImageFormat,
    name: String,
    purpose: Purpose,
    /// The picture as a bitmap stores it and what decoding converted, or
    /// the importer's refusal.
    read: Result<(Arc<Grayscale>, Vec<ImportWarning>), String>,
    /// The side last typed.
    side: PictureSide,
    /// Its length, mm; `None` until one is typed. Never defaulted: the
    /// file does not say how big the picture is on the stock.
    size_mm: Option<f64>,
}

impl PictureImport {
    /// The sheet over `bytes`, the file `name` (with its extension), a
    /// picture in `format` ([`FileKind::of`] reads it from the name),
    /// decoded at once as `image add` decodes it. The bytes are not kept.
    /// Decoding a large picture takes a moment: the shell may open the
    /// sheet on a worker thread.
    #[must_use]
    pub fn open(format: ImageFormat, name: &str, bytes: &[u8], purpose: Purpose) -> Self {
        Self {
            format,
            name: name.to_owned(),
            purpose,
            read: dc_import::bitmap::import(bytes, format)
                .map(|(pixels, warnings)| (Arc::new(pixels), warnings))
                .map_err(|e| e.to_string()),
            side: PictureSide::Width,
            size_mm: None,
        }
    }

    /// The file's format.
    #[must_use]
    pub fn format(&self) -> ImageFormat {
        self.format
    }

    /// The file's name, with its extension, as the undo label names it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The file's name without its extension: the bitmap's name, and a
    /// new project's, as `image add` and `init` name them.
    #[must_use]
    pub fn stem(&self) -> &str {
        stem(&self.name)
    }

    /// What *Import* does.
    #[must_use]
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }

    /// The picture as the bitmap will store it: 8-bit gray, top row
    /// first, for the sheet's preview. `None` when the file could not be
    /// decoded.
    #[must_use]
    pub fn pixels(&self) -> Option<&Arc<Grayscale>> {
        self.read.as_ref().ok().map(|(pixels, _)| pixels)
    }

    /// The picture's size, `(width_px, height_px)`.
    #[must_use]
    pub fn size_px(&self) -> Option<(u32, u32)> {
        self.pixels().map(|p| (p.width_px, p.height_px))
    }

    /// What decoding converted (16-bit samples, transparency, colour to
    /// grayscale), in the importer's order: what `image add` warns of.
    #[must_use]
    pub fn warnings(&self) -> &[ImportWarning] {
        self.read.as_ref().map_or(&[], |(_, warnings)| warnings)
    }

    /// [`PictureImport::warnings`] as the notes the Warnings panel lists
    /// after the import, each under the file's name with its code, as
    /// `image add` prints it: `photo.jpg: image_colour_converted: the
    /// picture is in colour; …`. The sheet shows them before the import.
    #[must_use]
    pub fn notes(&self, strings: &Strings) -> Vec<String> {
        self.warnings()
            .iter()
            .map(|warning| {
                strings.fmt(
                    StrKey::AppPictureNote,
                    &[
                        ("name", &self.name),
                        ("code", warning.code()),
                        ("note", &warning.to_string()),
                    ],
                )
            })
            .collect()
    }

    /// The side last typed; the width before any is.
    #[must_use]
    pub fn side(&self) -> PictureSide {
        self.side
    }

    /// The size typed, as `image add` takes it; `None` until one is.
    #[must_use]
    pub fn size(&self) -> Option<PictureSize> {
        let size_mm = self.size_mm?;
        Some(match self.side {
            PictureSide::Width => PictureSize::Width { width_mm: size_mm },
            PictureSide::Height => PictureSize::Height { height_mm: size_mm },
        })
    }

    /// Sets the footprint's width, mm; the height follows the pixels. A
    /// width that is not a positive length is `image add`'s refusal,
    /// shown as [`PictureImport::blocked`].
    pub fn set_width_mm(&mut self, width_mm: f64) {
        self.side = PictureSide::Width;
        self.size_mm = Some(width_mm);
    }

    /// Sets the footprint's height, mm; the width follows the pixels.
    pub fn set_height_mm(&mut self, height_mm: f64) {
        self.side = PictureSide::Height;
        self.size_mm = Some(height_mm);
    }

    /// Forgets the size, as when the field is emptied.
    pub fn clear_size(&mut self) {
        self.size_mm = None;
    }

    /// The footprint the picture gets, `(width_mm, height_mm)`: the side
    /// typed and the other in the pixels' proportion, as
    /// [`PictureSize::footprint_mm`] rounds it. `None` while *Import* is
    /// disabled.
    #[must_use]
    pub fn footprint_mm(&self) -> Option<(f64, f64)> {
        self.placed(self.purpose).ok()
    }

    /// Why *Import* is disabled: the importer's refusal of the file (the
    /// message `image add` prints after `cannot import <file>: `), that no
    /// size is typed yet, or the refusal of the size (`image add`'s for
    /// the open project, `init`'s for a new one, whose stock is the
    /// picture's with a margin). `None` when it is enabled.
    #[must_use]
    pub fn blocked(&self, strings: &Strings) -> Option<String> {
        match self.placed(self.purpose) {
            Ok(_) => None,
            Err(Refusal::Unreadable(message)) => Some(message.to_owned()),
            Err(Refusal::SizeMissing) => {
                Some(strings.get(StrKey::AppPictureSizeMissing).to_owned())
            }
            Err(Refusal::Placement(error)) => Some(error.to_string()),
        }
    }

    /// The command *Import* runs into the open project, when it is
    /// enabled: one undo step, *Import photo.png*, adding the bitmap
    /// `image add` adds at the stock's bottom-left corner.
    #[must_use]
    pub fn command(&self) -> Option<Command> {
        self.placed(Purpose::Import).ok()?;
        Some(Command::ImportPicture {
            name: self.name.clone(),
            pixels: Arc::clone(self.pixels()?),
            size: self.size()?,
        })
    }

    /// The footprint for `purpose`, checked as the function that will
    /// place the picture checks it, from the pixel counts alone.
    fn placed(&self, purpose: Purpose) -> Result<(f64, f64), Refusal<'_>> {
        let (pixels, _) = self
            .read
            .as_ref()
            .map_err(|message| Refusal::Unreadable(message))?;
        let size = self.size().ok_or(Refusal::SizeMissing)?;
        let (width_px, height_px) = (pixels.width_px, pixels.height_px);
        match purpose {
            Purpose::Import => {
                dc_job::picture::footprint_at(width_px, height_px, size, PICTURE_CORNER)
            }
            Purpose::New => dc_job::project_from::picture_stock(width_px, height_px, size)
                .and_then(|_| size.footprint_mm(width_px, height_px)),
        }
        .map_err(Refusal::Placement)
    }
}

/// Where *Add picture* puts a picture's bottom-left corner, project mm:
/// the stock's (capability 56, "at the origin"; `image add` without
/// `--at`).
pub(crate) const PICTURE_CORNER: Point = Point::new(0.0, 0.0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stem_drops_the_last_extension_only() {
        let sheet = |name: &str| {
            DrawingImport::open(DrawingFormat::Svg, name, Vec::new(), Purpose::New)
                .stem()
                .to_owned()
        };
        assert_eq!(sheet("coaster.svg"), "coaster");
        assert_eq!(sheet("my.plaque.dxf"), "my.plaque");
        assert_eq!(sheet(".svg"), ".svg");
        assert_eq!(sheet("README"), "README");
    }
}
