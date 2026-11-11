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

use std::collections::BTreeSet;

use dc_geometry::{Polyline, Rect};
use dc_import::{
    DxfImportOptions, DxfLayer, DxfUnits, ImportError, ImportWarning, JOIN_TOLERANCE_DEFAULT_MM,
    SvgImportOptions,
};
use dc_job::{Drawing, JobError};
use dc_project::Units;

use crate::command::Command;

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
        match self.name.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem,
            _ => &self.name,
        }
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
