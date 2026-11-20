// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/edge.ts
//   src/engine/toolpaths/geometry.ts
//   src/engine/toolpaths/drilling.ts
//   src/engine/toolpaths/carving.ts

//! The walk over an operation's targets (`docs/specs/phase-1/design.md`,
//! "Algorithms → Profile contour", step 1): every profile of every target
//! instance, in drawing order, for the generator to classify (closed,
//! open, a circle) and place.
//!
//! The walk was lifted out of the Profile, Engrave, Drill, V-Carve and
//! Inlay generators, which each wrote it by hand, so this file keeps their
//! files' upstream attribution. It places nothing itself: a generator
//! that skips a profile never placed it, so a profile it skips cannot fail
//! the operation. Nothing here depends on Y-up or Y-down.

use crate::error::{CamError, ShapeRef};
use dc_geometry::{Contour, GeometryError, Polyline};
use dc_project::{FeatureInstance, InstanceId, OperationId, Project};

/// One profile of one target instance, as drawn.
#[derive(Debug, Clone)]
pub(crate) struct TargetProfile<'p> {
    /// Which profile of which instance it is.
    pub shape: ShapeRef,
    /// The profile in its definition's space, mm, before the instance's
    /// transform.
    drawn: &'p Polyline,
    instance: &'p FeatureInstance,
}

impl TargetProfile<'_> {
    /// True when the profile is a closed outline.
    pub(crate) fn is_closed(&self) -> bool {
        self.drawn.is_closed()
    }

    /// The profile placed in project space by its instance's transform.
    ///
    /// # Errors
    ///
    /// The transform's own error, for a caller that reports it as it is.
    pub(crate) fn transformed(&self) -> Result<Polyline, GeometryError> {
        self.instance.transform.apply_polyline(self.drawn)
    }

    /// The profile placed in project space, then taken through `then`.
    ///
    /// # Errors
    ///
    /// [`CamError::InvalidTarget`], naming the profile, when the transform
    /// or `then` fails.
    pub(crate) fn placed_then<T>(
        &self,
        then: impl FnOnce(Polyline) -> Result<T, GeometryError>,
    ) -> Result<T, CamError> {
        self.transformed()
            .and_then(then)
            .map_err(|source| CamError::InvalidTarget {
                shape: self.shape.clone(),
                source,
            })
    }

    /// The profile placed in project space.
    ///
    /// # Errors
    ///
    /// [`CamError::InvalidTarget`] when it cannot be placed.
    pub(crate) fn placed(&self) -> Result<Polyline, CamError> {
        self.placed_then(Ok)
    }

    /// The profile placed in project space as a contour.
    ///
    /// # Errors
    ///
    /// [`CamError::InvalidTarget`] when it cannot be placed, or is not a
    /// closed outline that encloses area without crossing itself.
    pub(crate) fn contour(&self) -> Result<Contour, CamError> {
        self.placed_then(Contour::new)
    }
}

/// Every profile of every instance in `targets` of operation `op`, in
/// drawing order: target order, then profile order. An instance, or its
/// definition, that the project does not have is an `Err` item where the
/// walk reaches it, after the profiles of the targets before it
/// ([`CamError::UnknownInstance`], [`CamError::UnknownDefinition`]); a
/// caller stops there.
pub(crate) fn target_profiles<'p>(
    project: &'p Project,
    op: &'p OperationId,
    targets: &'p [InstanceId],
) -> impl Iterator<Item = Result<TargetProfile<'p>, CamError>> + 'p {
    targets.iter().flat_map(
        move |instance_id| match instance_profiles(project, op, instance_id) {
            Ok(profiles) => profiles.into_iter().map(Ok).collect(),
            Err(error) => vec![Err(error)],
        },
    )
}

/// The profiles of the instance `instance_id` names, in profile order.
fn instance_profiles<'p>(
    project: &'p Project,
    op: &OperationId,
    instance_id: &InstanceId,
) -> Result<Vec<TargetProfile<'p>>, CamError> {
    let instance = project
        .instances
        .iter()
        .find(|i| &i.id == instance_id)
        .ok_or_else(|| CamError::UnknownInstance {
            op: op.clone(),
            instance: instance_id.clone(),
        })?;
    let definition = project
        .definitions
        .get(&instance.definition)
        .ok_or_else(|| CamError::UnknownDefinition {
            instance: instance.id.clone(),
            definition: instance.definition.clone(),
        })?;
    Ok(definition
        .profiles
        .iter()
        .enumerate()
        .map(|(profile, drawn)| TargetProfile {
            shape: ShapeRef {
                instance: instance.id.clone(),
                instance_name: instance.name.clone(),
                profile,
            },
            drawn,
            instance,
        })
        .collect())
}
