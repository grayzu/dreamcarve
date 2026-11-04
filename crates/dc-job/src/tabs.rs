// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Placed tabs for a shell (`docs/specs/phase-4/design.md`, "Data model →
//! dc-job"; #105, ADR-0005): the `dc-cam` placement functions, re-exported
//! so the app reaches them through this crate (functional spec, S20), and
//! [`place_request`], what `dreamcarve tabs place <dcv> <op>` does with no
//! flags.

pub use dc_cam::placed_tabs::{
    Placement, Replacement, TabSize, along_outline_mm, crossed_by, freed_parts, off_outline_mm,
    on_holes, place, place_on_outlines, place_requests, replace, tab_point,
};
use dc_cam::{CamError, Warning};
use dc_project::{Operation, OperationId, PlacedTab, Project};

use crate::error::JobError;

/// `project` with the tab request of Profile `op` placed as tabs and
/// cleared, as `tabs place <dcv> <op>` saves it (functional spec,
/// acceptance 78): first every enabled Profile's request is placed
/// ([`place_requests`]), so the project holds placed tabs only, then a
/// disabled `op`'s own. The warnings are placing's, then `tab_on_hole` for
/// each new tab an enabled Profile cuts as a hole (#300, #302), as the CLI
/// prints them.
///
/// # Errors
///
/// [`JobError::NoTabRequest`] when `op` holds no request;
/// [`CamError::UnknownOperation`] and [`CamError::TabsNeedProfile`] when
/// `op` is not a Profile of `project`, and whatever placing fails with.
pub fn place_request(
    project: &Project,
    op: &OperationId,
) -> Result<(Project, Vec<Warning>), JobError> {
    let request = match project.operations.iter().find(|o| o.id() == op) {
        Some(Operation::Profile(profile)) => profile.tabs.clone(),
        Some(other) => {
            return Err(CamError::TabsNeedProfile {
                op: op.clone(),
                kind: other.kind(),
            }
            .into());
        }
        None => return Err(CamError::UnknownOperation { op: op.clone() }.into()),
    };
    let Some(spec) = request else {
        return Err(JobError::NoTabRequest { op: op.clone() });
    };
    let (mut placed, mut warnings) = place_requests(project)?;
    // An enabled Profile's request is placed above; a disabled one's only
    // when asked for by name.
    if let Some(Operation::Profile(profile)) = placed.operations.iter_mut().find(|o| o.id() == op)
        && profile.tabs.take().is_some()
    {
        let placement = place(&placed, op, &spec)?;
        warnings.extend(placement.warnings);
        placed.tabs.extend(placement.tabs);
    }
    let new: Vec<&PlacedTab> = placed
        .tabs
        .iter()
        .filter(|t| !project.tabs.iter().any(|r| r.id == t.id))
        .collect();
    warnings.extend(tabs_on_holes(&placed, &new));
    Ok((placed, warnings))
}

/// `tab_on_hole` once for each of `new` that an enabled Profile of
/// `project` cuts as a hole (#300). Generation does not warn again for a
/// tab placed on the hole with a request's `on_holes`, which saves it with
/// `holds_slug: true` (#302), so this is where the user hears that it
/// holds the slug.
#[must_use]
pub fn tabs_on_holes(project: &Project, new: &[&PlacedTab]) -> Vec<Warning> {
    let mut warnings = Vec::new();
    for op in &project.operations {
        let Operation::Profile(profile) = op else {
            continue;
        };
        if !profile.enabled {
            continue;
        }
        for (tab, shape) in on_holes(project, profile).unwrap_or_default() {
            if new.iter().any(|t| t.id == tab) {
                warnings.push(Warning::TabOnHole {
                    op: profile.id.clone(),
                    tab,
                    shape,
                });
            }
        }
    }
    warnings
}
