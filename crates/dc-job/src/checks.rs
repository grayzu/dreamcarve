// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The artwork checks as both shells report them (phase 3 functional
//! spec, capability 49a; phase 4 functional spec, capabilities 55 and
//! 76): what in a drawing trips a V-Carve or an Inlay. The rules, the
//! thresholds, the codes and the words are `dc_import::prep`'s; this
//! module owns which of them are read from what, so `dreamcarve init`,
//! `dreamcarve check`, the app's Import drawing sheet and its Warnings
//! drawer call one function each and say the same thing:
//!
//! - [`svg`]: every check, read from an SVG's paint and paths, which
//!   `init` prints and the sheet lists.
//! - [`project`]: the two that can still be read from a project's shapes
//!   (`shapes_overlap`, `feature_below_size`), which `check` prints and
//!   the drawer lists; moved from `crates/dreamcarve/src/check.rs` with
//!   its findings unchanged.
//! - [`drawing`]: the same two for a drawing before it is in a project,
//!   which is how the sheet reads a DXF's (a DXF has no paint).
//!
//! Nothing here changes a drawing or refuses one: a check that fired is a
//! warning.

use dc_geometry::{Contour, Region, nest};
use dc_import::{ImportError, ImportWarning, SvgImportOptions, prep};
use dc_project::{FeatureDefinition, FeatureInstance, InstanceId, Project};

use crate::build::Tier;
use crate::import::Drawing;

/// The code of the one line that stands in for an SVG's checks when they
/// could not be run ([`svg`]).
pub const SKIPPED: &str = "import_checks_skipped";

/// One artwork check that fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Its stable `snake_case` code (phase 3 functional, 49g).
    pub code: &'static str,
    /// What it found, where, and what to do: the words the CLI prints
    /// after the code.
    pub message: String,
    /// The shapes the message names, when the check was read from shapes
    /// that have ids ([`project`]): the first pair of `shapes_overlap`.
    /// Empty for a check that names none, and for an SVG's ([`svg`]),
    /// whose paths are not shapes yet.
    pub shapes: Vec<InstanceId>,
}

impl Check {
    /// `warning` as a check that names no shape.
    fn of(warning: &ImportWarning) -> Self {
        Self {
            code: warning.code(),
            message: warning.to_string(),
            shapes: Vec::new(),
        }
    }

    /// How much it matters, by its code ([`Tier::of_warning_code`]): a
    /// warning, as the CLI prints every one of them.
    #[must_use]
    pub fn tier(&self) -> Tier {
        Tier::of_warning_code(self.code)
    }

    /// The line `init` and `check` print for it:
    /// `warning: <code>: <message>`.
    #[must_use]
    pub fn line(&self) -> String {
        format!("warning: {}: {}", self.code, self.message)
    }

    /// Whether the check is one [`project`] reads again from the shapes
    /// once the drawing is imported, so a shell that keeps the others as
    /// notes of the import need not keep this one.
    #[must_use]
    pub fn read_from_shapes(&self) -> bool {
        matches!(self.code, "shapes_overlap" | "feature_below_size")
    }

    /// Whether the check says the import left part of the drawing out or
    /// changed it: text and images skipped, clip paths and masks ignored
    /// (`svg_object_not_convertible`). It stands in for the importer's own
    /// warning of that ([`counted_by_svg_checks`]). The other checks of a
    /// drawing's paint (a hole drawn as a separate path, a stroke with no
    /// fill, a hidden object) are advice about shapes that were imported
    /// whole, or about a layer the drawing itself hides: an outline drawn as a hairline, which is how most drawings
    /// for a cut-out are made, is one of them and needs no fix.
    #[must_use]
    pub fn says_something_was_left_out(&self) -> bool {
        self.code == "svg_object_not_convertible"
    }
}

/// Whether `warning`, one of the SVG importer's, is counted by the checks
/// over the whole document (text, images, clip paths and masks), so the
/// check's line stands in for it and a shell that shows both leaves the
/// importer's out, as `init` does.
#[must_use]
pub fn counted_by_svg_checks(warning: &ImportWarning) -> bool {
    matches!(
        warning,
        ImportWarning::TextIgnored { .. }
            | ImportWarning::ImagesIgnored { .. }
            | ImportWarning::ClipPathIgnored { .. }
            | ImportWarning::MaskIgnored { .. }
    )
}

/// The 49a checks of the SVG in `bytes`, in `dc_import::prep::findings`'
/// order, at the importer's default options: what `init` prints after the
/// importer's own warnings.
///
/// # Errors
///
/// The one [`SKIPPED`] check when the drawing could not be measured (the
/// checks budget the strokes' outlines, which the import does not build):
/// the checks never change the outcome, so the drawing is imported all
/// the same and this line says they did not run.
pub fn svg(bytes: &[u8]) -> Result<Vec<Check>, Check> {
    match prep::findings(bytes, &SvgImportOptions::new()) {
        Ok(found) => Ok(found.iter().map(Check::of).collect()),
        Err(e) => Err(skipped(&e)),
    }
}

/// The line that says the checks did not run, and why.
fn skipped(error: &ImportError) -> Check {
    Check {
        code: SKIPPED,
        message: format!("the drawing could not be checked: {error}"),
        shapes: Vec::new(),
    }
}

/// The two checks that still apply to a project (`shapes_overlap`,
/// `feature_below_size`), read from the regions of its visible shapes
/// with the design's height from their joint bounding box (phase 3
/// functional, 49a): what `check` prints after its warnings. Shapes that
/// do not resolve into regions are left out; validation reports them.
/// `shapes_overlap` carries the ids of the pair it names.
#[must_use]
pub fn project(project: &Project) -> Vec<Check> {
    of_shapes(
        project
            .instances
            .iter()
            .filter(|instance| instance.visible)
            .filter_map(|instance| {
                Some((instance, project.definitions.get(&instance.definition)?))
            }),
    )
}

/// [`project`]'s checks for `drawing` as imported, before it is in a
/// project: the shapes an import would add, placed as drawn. The ids are
/// the importer's (`i1`, …), which the project renumbers.
#[must_use]
pub fn drawing(drawing: &Drawing) -> Vec<Check> {
    let (definitions, instances) = match drawing {
        Drawing::Svg(import) => (&import.definitions, &import.instances),
        Drawing::Dxf(import) => (&import.definitions, &import.instances),
    };
    of_shapes(
        instances
            .iter()
            .filter(|instance| instance.visible)
            .filter_map(|instance| {
                let definition = definitions.iter().find(|d| d.id == instance.definition)?;
                Some((instance, definition))
            }),
    )
}

/// The checks of `shapes`, each an instance with its definition, in
/// order.
fn of_shapes<'a>(
    shapes: impl Iterator<Item = (&'a FeatureInstance, &'a FeatureDefinition)>,
) -> Vec<Check> {
    let mut placed: Vec<&FeatureInstance> = Vec::new();
    // Each shape under its place in `placed`, not its name: two shapes
    // may share a name, and the check must say which pair it found.
    let mut regions_of: Vec<(String, Vec<Region>)> = Vec::new();
    for (instance, definition) in shapes {
        let contours: Option<Vec<Contour>> = definition
            .profiles
            .iter()
            .filter(|p| p.is_closed())
            .map(|p| {
                let outline = instance.transform.apply_polyline(p).ok()?;
                Contour::new(outline).ok()
            })
            .collect();
        if let Some(Ok((regions, _))) = contours.map(nest)
            && !regions.is_empty()
        {
            regions_of.push((placed.len().to_string(), regions));
            placed.push(instance);
        }
    }
    let bounds = regions_of
        .iter()
        .flat_map(|(_, regions)| regions)
        .filter_map(|r| r.outer().polyline().bounds())
        .reduce(|a, b| a.including(b.min).including(b.max));
    let Some(bounds) = bounds else {
        return Vec::new();
    };
    let shape_at = |place: &str| {
        place
            .parse::<usize>()
            .ok()
            .and_then(|k| placed.get(k).copied())
    };
    prep::design_findings_named(&regions_of, bounds.height_mm())
        .into_iter()
        .map(|warning| match warning {
            ImportWarning::ShapesOverlap {
                count,
                first,
                second,
                x_mm,
                y_mm,
            } => {
                let pair = [shape_at(&first), shape_at(&second)];
                let name = |shape: Option<&FeatureInstance>, place: String| {
                    shape.map_or(place, |instance| instance.name.clone())
                };
                // The message with the shapes' names, as `check` words it.
                let named = ImportWarning::ShapesOverlap {
                    count,
                    first: name(pair[0], first),
                    second: name(pair[1], second),
                    x_mm,
                    y_mm,
                };
                Check {
                    shapes: pair
                        .iter()
                        .flatten()
                        .map(|instance| instance.id.clone())
                        .collect(),
                    ..Check::of(&named)
                }
            }
            other => Check::of(&other),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "tests"
    )]

    use super::*;
    use dc_import::{DxfImportOptions, import_dxf, import_svg};

    const MESSY_SVG: &[u8] = include_bytes!("../../../tests/fixtures/prep/messy.svg");
    const MESSY_DCV: &[u8] = include_bytes!("../../../tests/fixtures/prep/messy.dcv");
    const CLEAN_SVG: &[u8] = include_bytes!("../../../tests/fixtures/prep/clean.svg");
    const TRAY_DXF: &[u8] = include_bytes!("../../../tests/fixtures/prep/tool-tray.dxf");

    fn codes(checks: &[Check]) -> Vec<&'static str> {
        checks.iter().map(|c| c.code).collect()
    }

    /// An SVG's checks are `prep::findings`' warnings, one check each,
    /// in its order, with its code and its words; a clean drawing has
    /// none.
    #[test]
    fn an_svgs_checks_are_the_importers_findings() {
        let found = prep::findings(MESSY_SVG, &SvgImportOptions::new()).unwrap();
        let checks = svg(MESSY_SVG).unwrap();
        assert_eq!(checks.len(), found.len());
        for (check, warning) in checks.iter().zip(&found) {
            assert_eq!(check.code, warning.code());
            assert_eq!(check.message, warning.to_string());
            assert_eq!(
                check.line(),
                format!("warning: {}: {warning}", warning.code())
            );
            assert_eq!(check.tier(), Tier::Warning, "{}", check.code);
            assert!(check.shapes.is_empty(), "an SVG's paths have no ids yet");
            assert_eq!(
                check.says_something_was_left_out(),
                matches!(warning, ImportWarning::SvgObjectNotConvertible { .. }),
                "{}",
                check.code
            );
        }
        assert_eq!(svg(CLEAN_SVG), Ok(Vec::new()));
    }

    /// A drawing the checks cannot measure is not refused: one line says
    /// they did not run, with the importer's reason.
    #[test]
    fn checks_that_cannot_run_are_one_line() {
        let huge = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100"><path d="M0,0 L50000,0" fill="none" stroke="black" stroke-width="50" stroke-dasharray="0.001"/></svg>"#;
        let reason = prep::findings(huge, &SvgImportOptions::new()).unwrap_err();
        let check = svg(huge).unwrap_err();
        assert_eq!(check.code, SKIPPED);
        assert_eq!(
            check.line(),
            format!("warning: import_checks_skipped: the drawing could not be checked: {reason}")
        );
    }

    /// The importer's four warnings the checks count over the whole
    /// document are told from the rest.
    #[test]
    fn the_checks_stand_in_for_text_images_clips_and_masks() {
        let (_, warnings) = import_svg(MESSY_SVG, &SvgImportOptions::new()).unwrap();
        assert!(!warnings.is_empty());
        for warning in &warnings {
            assert_eq!(
                counted_by_svg_checks(warning),
                warning.code() == "svg_object_not_convertible",
                "{warning:?}"
            );
        }
    }

    /// The messy project's two squares overlap and its dot is a speck:
    /// the two checks a project's shapes can still be read for, the
    /// overlap with the ids of the pair it names.
    #[test]
    fn a_projects_checks_name_the_shapes_by_id() {
        let project_ = dc_project::load(MESSY_DCV).unwrap().project;
        let checks = project(&project_);
        assert_eq!(codes(&checks), ["shapes_overlap", "feature_below_size"]);
        let named: Vec<&str> = checks[0]
            .shapes
            .iter()
            .map(|id| {
                project_
                    .instances
                    .iter()
                    .find(|i| &i.id == id)
                    .unwrap()
                    .name
                    .as_str()
            })
            .collect();
        assert_eq!(named, ["sq-a", "sq-b"]);
        assert!(
            checks[0].message.contains("the first is `sq-a` and `sq-b`"),
            "{}",
            checks[0].message
        );
        assert!(checks[1].shapes.is_empty());
        assert!(checks.iter().all(Check::read_from_shapes));
    }

    /// Two shapes of one name are told apart: the pair named is the pair
    /// that overlaps, not the first shapes of that name. The squares are
    /// put last, so a look-up by name would find other shapes.
    #[test]
    fn shapes_of_one_name_are_told_apart() {
        let mut project_ = dc_project::load(MESSY_DCV).unwrap().project;
        project_.instances.reverse();
        let squares: Vec<InstanceId> = project_
            .instances
            .iter()
            .filter(|i| i.name == "sq-a" || i.name == "sq-b")
            .map(|i| i.id.clone())
            .collect();
        assert_eq!(squares.len(), 2);
        for instance in &mut project_.instances {
            instance.name = "shape".to_owned();
        }
        let checks = project(&project_);
        let overlap = &checks[0];
        assert!(
            overlap.message.contains("the first is `shape` and `shape`"),
            "{}",
            overlap.message
        );
        assert_eq!(overlap.shapes, squares);
    }

    /// A hidden shape is not checked, as `check` leaves it out.
    #[test]
    fn hidden_shapes_are_left_out() {
        let mut project_ = dc_project::load(MESSY_DCV).unwrap().project;
        for instance in &mut project_.instances {
            if instance.name == "sq-b" {
                instance.visible = false;
            }
        }
        assert_eq!(codes(&project(&project_)), ["feature_below_size"]);
    }

    /// The tray's two pockets overlap and it has a speck: a DXF's checks
    /// are read from the shapes its import would add, and are what
    /// `check` finds in the project `init` makes of them (the overlap's
    /// place apart, which the stock's margin moves).
    #[test]
    fn a_drawings_checks_are_its_projects() {
        let (import, _) = import_dxf(TRAY_DXF, &DxfImportOptions::new()).unwrap();
        let read = Drawing::Dxf(import);
        let checks = drawing(&read);
        assert_eq!(codes(&checks), ["shapes_overlap", "feature_below_size"]);
        assert!(
            checks[0]
                .message
                .contains("the first is `pockets-1` and `pockets-2`"),
            "{}",
            checks[0].message
        );
        let post = dc_project::latest_definition("grblhal")
            .unwrap()
            .default_ref();
        let made =
            crate::project_from::drawing("tray", read, post, dc_project::Units::Mm, true).unwrap();
        let of_project = project(&made);
        assert_eq!(codes(&of_project), codes(&checks));
        assert!(
            of_project[0]
                .message
                .contains("the first is `pockets-1` and `pockets-2`"),
            "{}",
            of_project[0].message
        );
        assert_eq!(of_project[1].message, checks[1].message);
        assert_eq!(of_project[0].shapes.len(), 2);
    }
}
