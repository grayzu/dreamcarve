// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The commands that write a shape's geometry
//! (`docs/specs/phase-5/design.md`, "Data model → dc-app"): a drawn shape
//! added, as a new instance of a new definition (functional capability
//! 90).

use dc_geometry::{Polyline, Transform};
use dc_project::{
    DefinitionId, FeatureDefinition, FeatureInstance, FolderId, InstanceId, Project, Shape,
};

use super::next_free_id;
use crate::error::CommandError;

/// A shape to add: an instance and the definition it places.
#[derive(Debug, Clone, PartialEq)]
pub struct NewShape {
    /// The instance's and the definition's name: *Rectangle 1*.
    pub name: String,
    /// How the outline was made, for a shape that keeps its sizes or
    /// its curves (ADR-0006); `None` for a plain path.
    pub shape: Option<Shape>,
    /// The outline in definition space, mm: what is cut.
    pub profiles: Vec<Polyline>,
    /// Where the instance is placed.
    pub transform: Transform,
    /// Construction geometry: drawn and snapped to, never cut.
    pub construction: bool,
    /// The folder the instance is filed in.
    pub folder: Option<FolderId>,
}

/// `project` with `shapes` added, each a new instance at the end of the
/// Design Tree with a definition of its own. Ids are the next free
/// `d<n>` and `i<n>`.
///
/// # Errors
///
/// [`CommandError::NothingToDo`] for no shapes or a shape with no
/// outline; [`CommandError::Geometry`] when the placed outline cannot be
/// (a vertex that is not a number, a reach past the coordinate limit, an
/// arc under an uneven scale); [`CommandError::UnknownFolder`] for a
/// folder the project does not have.
pub(super) fn add(project: &Project, shapes: &[NewShape]) -> Result<Project, CommandError> {
    if shapes.is_empty() || shapes.iter().any(|shape| shape.profiles.is_empty()) {
        return Err(CommandError::NothingToDo);
    }
    let mut next = project.clone();
    for shape in shapes {
        // A project every reader would refuse is refused here.
        for profile in &shape.profiles {
            shape.transform.apply_polyline(profile)?;
        }
        if let Some(folder) = &shape.folder
            && !next.folders.iter().any(|f| &f.id == folder)
        {
            return Err(CommandError::UnknownFolder(folder.clone()));
        }
        let definitions: Vec<&str> = next.definitions.keys().map(DefinitionId::as_str).collect();
        let definition = DefinitionId::new(next_free_id(&definitions, "d"));
        let instances: Vec<&str> = next.instances.iter().map(|i| i.id.as_str()).collect();
        let instance = InstanceId::new(next_free_id(&instances, "i"));
        next.definitions.insert(
            definition.clone(),
            FeatureDefinition {
                id: definition.clone(),
                name: shape.name.clone(),
                shape: shape.shape.clone(),
                profiles: shape.profiles.clone(),
            },
        );
        next.instances.push(FeatureInstance {
            id: instance,
            definition,
            transform: shape.transform,
            name: shape.name.clone(),
            visible: true,
            locked: false,
            construction: shape.construction,
            folder: shape.folder.clone(),
            z_top_mm: None,
            z_bottom_mm: None,
        });
    }
    Ok(next)
}

/// `<base> <n>` for the least `n` from 1 that no instance of `project`
/// is named (capability 90): *Rectangle 1*, then *Rectangle 2*, and
/// *Rectangle 1* again once the first is deleted or renamed.
#[must_use]
pub fn next_name(project: &Project, base: &str) -> String {
    (1u32..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !project.instances.iter().any(|i| &i.name == name))
        .unwrap_or_else(|| base.to_owned())
}
