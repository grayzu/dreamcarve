// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The commands that write a shape's geometry
//! (`docs/specs/phase-5/design.md`, "Data model → dc-app"): a drawn shape
//! added, as a new instance of a new definition (functional capability
//! 90); a definition's outline replaced, with the tabs on it re-seated;
//! a shape that keeps its sizes made a plain path (capability 84).

use dc_geometry::{Point, Polyline, Rect, Transform, nearest_point};
use dc_project::{
    DefinitionId, FeatureDefinition, FeatureInstance, FolderId, InstanceId, Project, Shape,
};

use super::instances::{bounds, find};
use super::next_free_id;
use crate::error::CommandError;
use crate::tool::SizeField;

/// What a change of a shape's outline was, for its undo label: *Set
/// **width** of “Slot 1”*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// One of its sizes.
    Size(SizeField),
    /// A rectangle's corners: square, rounded or chamfered.
    Corners,
    /// What a polygon's radius reaches.
    Measured,
    /// The form of a gear's flanks.
    Flanks,
    /// The form of a gear's root.
    Root,
    /// The form of a gear's crest.
    Crest,
}

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

/// Whether `shape` is a shape that keeps its sizes: anything but a path,
/// which keeps its nodes.
pub(crate) const fn keeps_sizes(shape: &Shape) -> bool {
    !matches!(shape, Shape::Path { .. })
}

/// `SetOutline`: the definition `instance` places takes `shape` and
/// `profiles`, for every instance of it, and the tabs on them are
/// re-seated on the new outline.
///
/// # Errors
///
/// [`CommandError::Locked`] naming every locked instance of the
/// definition; [`CommandError::EmptyOutline`] for no outline;
/// [`CommandError::NothingToDo`] for the shape and outline it has; and
/// [`CommandError::Geometry`] when an instance cannot place the outline.
pub(super) fn set_outline(
    project: &Project,
    instance: &InstanceId,
    shape: Option<&Shape>,
    profiles: &[Polyline],
) -> Result<Project, CommandError> {
    let definition = find(project, instance)?.definition.clone();
    let copies: Vec<&FeatureInstance> = project
        .instances
        .iter()
        .filter(|i| i.definition == definition)
        .collect();
    let locked: Vec<String> = copies
        .iter()
        .filter(|i| i.locked)
        .map(|i| i.name.clone())
        .collect();
    if !locked.is_empty() {
        return Err(CommandError::Locked { names: locked });
    }
    if profiles.is_empty() {
        return Err(CommandError::EmptyOutline);
    }
    let ids: Vec<InstanceId> = copies.iter().map(|i| i.id.clone()).collect();
    let mut next = project.clone();
    let slot =
        next.definitions
            .get_mut(&definition)
            .ok_or_else(|| CommandError::UnknownDefinition {
                instance: instance.clone(),
                definition: definition.clone(),
            })?;
    if slot.shape.as_ref() == shape && slot.profiles == profiles {
        return Err(CommandError::NothingToDo);
    }
    let old = std::mem::replace(&mut slot.profiles, profiles.to_vec());
    slot.shape = shape.cloned();
    reseat_tabs(&mut next, &ids, &old, profiles);
    // An outline no reader would take (a vertex that is not a number, a
    // reach past the coordinate limit) is refused here.
    bounds(&next, &ids)?;
    Ok(next)
}

/// The joint box of `profiles`, definition space.
fn joint_box(profiles: &[Polyline]) -> Option<Rect> {
    profiles
        .iter()
        .filter_map(Polyline::bounds)
        .reduce(|joint, rect| joint.including(rect.min).including(rect.max))
}

/// Moves the tabs on `instances` from the outline `old` to `new`, which
/// replaces it: each to the point of the new outline nearest the place
/// that is the same share of the shape's box as its old place was, on
/// the profile of its index, or on the nearest profile when that index is
/// gone. A tab is lost only when there is no outline left.
pub(super) fn reseat_tabs(
    project: &mut Project,
    instances: &[InstanceId],
    old: &[Polyline],
    new: &[Polyline],
) {
    if new.is_empty() {
        project
            .tabs
            .retain(|tab| !instances.contains(&tab.instance));
        return;
    }
    let boxes = joint_box(old).zip(joint_box(new));
    // One axis: the same share of the new extent, or, where the old
    // outline had no extent, the same distance from the middle.
    let carried = |at: f64, was: (f64, f64), is: (f64, f64)| {
        let (was_extent, is_extent) = (was.1 - was.0, is.1 - is.0);
        if was_extent > dc_geometry::EPS_MM {
            is.0 + (at - was.0) / was_extent * is_extent
        } else {
            at - (was.0 + was.1) / 2.0 + (is.0 + is.1) / 2.0
        }
    };
    for tab in &mut project.tabs {
        if !instances.contains(&tab.instance) {
            continue;
        }
        let at = boxes.map_or(Point::new(tab.x_mm, tab.y_mm), |(was, is)| {
            Point::new(
                carried(tab.x_mm, (was.min.x, was.max.x), (is.min.x, is.max.x)),
                carried(tab.y_mm, (was.min.y, was.max.y), (is.min.y, is.max.y)),
            )
        });
        let on = |index: usize| {
            new.get(index)
                .and_then(|profile| nearest_point(profile, at))
        };
        let seat = on(tab.profile).map(|hit| (tab.profile, hit)).or_else(|| {
            (0..new.len())
                .filter_map(|index| on(index).map(|hit| (index, hit)))
                .min_by(|a, b| a.1.gap_mm.total_cmp(&b.1.gap_mm))
        });
        if let Some((index, hit)) = seat {
            tab.profile = index;
            tab.x_mm = hit.point.x;
            tab.y_mm = hit.point.y;
        }
    }
}

/// `ConvertToPath`: each of `ids` whose shape keeps its sizes becomes a
/// plain path with the same outline, and so does every linked copy of it.
///
/// # Errors
///
/// [`CommandError::NothingSelected`] for no ids, an unknown instance, and
/// [`CommandError::NothingToDo`] when none keeps sizes.
pub(super) fn convert_to_path(
    project: &Project,
    ids: &[InstanceId],
) -> Result<Project, CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    let mut next = project.clone();
    let mut converted = false;
    for id in ids {
        let definition = &find(project, id)?.definition;
        if let Some(slot) = next.definitions.get_mut(definition)
            && slot.shape.as_ref().is_some_and(keeps_sizes)
        {
            slot.shape = None;
            converted = true;
        }
    }
    if converted {
        Ok(next)
    } else {
        Err(CommandError::NothingToDo)
    }
}

/// The instances of `ids` whose shape keeps its sizes, in `project`.
pub(super) fn sized<'a>(project: &'a Project, ids: &'a [InstanceId]) -> Vec<&'a InstanceId> {
    ids.iter()
        .filter(|id| {
            find(project, id)
                .ok()
                .and_then(|instance| project.definitions.get(&instance.definition))
                .and_then(|definition| definition.shape.as_ref())
                .is_some_and(keeps_sizes)
        })
        .collect()
}
