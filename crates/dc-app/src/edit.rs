// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/helpers/profileEdit.ts (applyLineCornerFillet)

//! The commands that change a shape's outline as a whole
//! (`docs/specs/phase-5/design.md`, "Module layout"; functional
//! capabilities 93 to 97): **Shape ▸ Fillet Corners…** here, and the
//! offset, the booleans, trim and join as their slices arrive. Each is a
//! pure function of the project that gives the one command to run, as
//! `dc_app::properties` and `dc_app::shape` do.
//!
//! A shape's outline is read as paths ([`paths_of`]): its own, where it
//! keeps curves, else each profile exactly. A shape that keeps its sizes
//! is no path until it is converted (capability 84).

use dc_geometry::{CornerError, Path, Polyline};
use dc_project::{FeatureDefinition, InstanceId, Project, Shape, Units};

use crate::command::{Command, Edit, find_instance};
use crate::error::CommandError;
use crate::strings::{StrKey, Strings};
use crate::units;

/// The paths a definition's outline is: its own, when it keeps curves,
/// else each profile as a path. `None` for a shape that keeps its sizes.
pub(crate) fn paths_of(definition: &FeatureDefinition) -> Option<Vec<Path>> {
    match &definition.shape {
        Some(Shape::Path { paths }) => Some(paths.clone()),
        Some(_) => None,
        None => Some(
            definition
                .profiles
                .iter()
                .map(Path::from_polyline)
                .collect(),
        ),
    }
}

/// The outline `paths` give, flattened to `tolerance_mm`, and the shape
/// that keeps them: the paths where one has a curve, else nothing, since
/// lines and arcs are the outline itself (ADR-0006).
pub(crate) fn outline_of(paths: Vec<Path>, tolerance_mm: f64) -> (Option<Shape>, Vec<Polyline>) {
    let profiles = paths
        .iter()
        .map(|path| path.to_polyline(tolerance_mm))
        .collect();
    let shape = paths
        .iter()
        .any(Path::has_curves)
        .then_some(Shape::Path { paths });
    (shape, profiles)
}

/// Why a corner was left as it is, a limit in it in project units and
/// `scale` times as the shape is placed.
pub(crate) fn corner_reason(
    strings: &Strings,
    units: Units,
    error: CornerError,
    scale: f64,
) -> String {
    match error {
        CornerError::TooLarge { max_mm } => strings.fmt(
            StrKey::NodeRefusedCornerTooLarge,
            &[("max", &units::limit_text(max_mm * scale, units))],
        ),
        other => other.to_string(),
    }
}

/// Every corner of `paths` rounded by `radius_mm` that can take it
/// (capability 93): the paths, how many corners were rounded, and why
/// the first that was not was left.
pub(crate) fn fillet_all(
    paths: &[Path],
    radius_mm: f64,
) -> (Vec<Path>, usize, Option<CornerError>) {
    let mut out = Vec::with_capacity(paths.len());
    let mut rounded = 0;
    let mut left = None;
    for path in paths {
        let mut path = path.clone();
        // From the last node back, so the nodes before keep their
        // numbers as each corner becomes two nodes.
        for node in (0..path.nodes().len()).rev() {
            match path.fillet(node, radius_mm) {
                Ok(filleted) => {
                    path = filleted;
                    rounded += 1;
                }
                // An open path's end is no corner to speak of.
                Err(CornerError::End) => {}
                Err(error) => left = left.or(Some(error)),
            }
        }
        out.push(path);
    }
    (out, rounded, left)
}

/// **Shape ▸ Fillet Corners…** on shape `id` with the radius typed as
/// `text`: every corner between two straight sides that can take the
/// radius is rounded, the way an inside pocket is prepared for a round
/// tool. One `SetOutline`.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is no length above 0,
/// when the shape keeps its sizes (it is converted to a path first), and
/// when no corner can take the radius, with the reason.
pub fn fillet_corners(
    project: &Project,
    strings: &Strings,
    id: &InstanceId,
    text: &str,
) -> Result<Command, CommandError> {
    let refused = |reason: String| CommandError::NotAQuantity {
        text: text.trim().to_owned(),
        reason,
    };
    let instance = find_instance(project, id)?;
    let units = project.display_units;
    let radius_mm = units::parse_length_mm(text, units)?;
    if radius_mm <= 0.0 {
        return Err(refused(
            strings.get(StrKey::DrawProblemNotPositive).to_owned(),
        ));
    }
    let Some(paths) = project
        .definitions
        .get(&instance.definition)
        .and_then(paths_of)
    else {
        return Err(refused(
            strings.fmt(StrKey::NodePromptSized, &[("name", &instance.name)]),
        ));
    };
    // The radius is the one seen: as the shape is placed.
    let Some(scale) = crate::shape::even_scale(instance.transform) else {
        return Err(CommandError::Stretched {
            name: instance.name.clone(),
        });
    };
    let (paths, rounded, left) = fillet_all(&paths, radius_mm / scale);
    if rounded == 0 {
        let reason = left.map_or_else(
            || strings.get(StrKey::NodeRefusedNoCorner).to_owned(),
            |error| corner_reason(strings, units, error, scale),
        );
        return Err(refused(reason));
    }
    let (shape, profiles) = outline_of(paths, project.flatten_tolerance_mm);
    Ok(Command::SetOutline {
        instance: id.clone(),
        shape,
        profiles,
        what: Edit::Fillet { count: rounded },
    })
}
