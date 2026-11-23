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
    /// Nodes of a path moved (capability 92): by a drag, a nudge or
    /// typed coordinates.
    MoveNodes {
        /// How many.
        count: usize,
    },
    /// A curve's handle or an arc's middle dragged, or an arc's radius
    /// typed: the outline between two nodes changed.
    Reshape,
    /// A node added on a span.
    AddNode,
    /// Nodes deleted, their neighbours joined.
    DeleteNodes {
        /// How many.
        count: usize,
    },
    /// A path broken at a node.
    Break,
    /// A span deleted.
    DeleteSegment,
    /// An open path closed by its own two ends.
    CloseEnds,
    /// Two open paths joined at their ends into one.
    JoinEnds,
}

/// The undo label of `what` done to `subject`, whose shape was `shape`.
pub(super) fn edit_label(
    strings: &crate::strings::Strings,
    what: Edit,
    shape: Option<&Shape>,
    subject: &str,
) -> String {
    use crate::strings::StrKey;
    let with = |key: StrKey| strings.fmt(key, &[("subject", subject)]);
    let counted = |key: StrKey, count: usize| {
        strings.fmt(key, &[("count", &count.to_string()), ("subject", subject)])
    };
    match what {
        Edit::MoveNodes { count: 1 } => with(StrKey::CommandLabelMoveNode),
        Edit::MoveNodes { count } => counted(StrKey::CommandLabelMoveNodes, count),
        Edit::Reshape => with(StrKey::CommandLabelReshape),
        Edit::AddNode => with(StrKey::CommandLabelAddNode),
        Edit::DeleteNodes { count: 1 } => with(StrKey::CommandLabelDeleteNode),
        Edit::DeleteNodes { count } => counted(StrKey::CommandLabelDeleteNodes, count),
        Edit::Break => with(StrKey::CommandLabelBreakPath),
        Edit::DeleteSegment => with(StrKey::CommandLabelDeleteSegment),
        Edit::CloseEnds => with(StrKey::CommandLabelClosePath),
        Edit::JoinEnds => with(StrKey::CommandLabelJoinPaths),
        Edit::Size(_)
        | Edit::Corners
        | Edit::Measured
        | Edit::Flanks
        | Edit::Root
        | Edit::Crest => strings.fmt(
            StrKey::CommandLabelSetField,
            &[
                ("field", strings.get(crate::shape::edit_word(shape, what))),
                ("subject", subject),
            ],
        ),
    }
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

/// `ReplaceShapes`: the instances `remove` go and the shapes `add` take
/// their place in the Design Tree, each a new instance of a new
/// definition. An operation that targeted `inherit`, and an Inlay that
/// had it in its boundary, has the first new shape in its place; the
/// other removed shapes leave every target list. The tabs on `inherit`
/// move to the first new shape, re-seated on its outline (it is placed as
/// `inherit` was); the tabs on the others go.
///
/// # Errors
///
/// [`CommandError::NothingSelected`] for nothing to remove,
/// [`CommandError::NothingToDo`] for nothing to add,
/// [`CommandError::Locked`] when a removed shape is locked, and
/// [`add`]'s for a shape that cannot be.
pub(super) fn replace(
    project: &Project,
    remove: &[InstanceId],
    new: &[NewShape],
    inherit: Option<&InstanceId>,
) -> Result<Project, CommandError> {
    if remove.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    let mut locked = Vec::new();
    for id in remove {
        let instance = find(project, id)?;
        if instance.locked {
            locked.push(instance.name.clone());
        }
    }
    if !locked.is_empty() {
        return Err(CommandError::Locked { names: locked });
    }
    let position = project
        .instances
        .iter()
        .position(|i| remove.contains(&i.id))
        .unwrap_or(project.instances.len());
    let mut next = add(project, new)?;
    let added: Vec<FeatureInstance> = next.instances.split_off(project.instances.len());
    let first = added.first().map(|i| i.id.clone());
    // What cut or bounded the inherited shape cuts or bounds the first
    // new one.
    let retarget = |targets: &mut Vec<InstanceId>| {
        let mut kept = Vec::with_capacity(targets.len());
        for target in targets.drain(..) {
            if Some(&target) == inherit {
                kept.extend(first.clone());
            } else if !remove.contains(&target) {
                kept.push(target);
            }
        }
        *targets = kept;
    };
    for operation in &mut next.operations {
        if let Some(targets) = super::operations::targets_mut(operation) {
            retarget(targets);
        }
        if let dc_project::Operation::Inlay(inlay) = operation {
            retarget(&mut inlay.boundary);
        }
    }
    // The inherited shape's tabs go with its outline.
    let old_profiles = inherit
        .and_then(|id| find(project, id).ok())
        .and_then(|instance| project.definitions.get(&instance.definition))
        .map(|definition| definition.profiles.clone());
    next.tabs
        .retain(|tab| Some(&tab.instance) == inherit || !remove.contains(&tab.instance));
    if let (Some(inherit), Some(first), Some(old), Some(shape)) =
        (inherit, &first, old_profiles, new.first())
    {
        for tab in &mut next.tabs {
            if &tab.instance == inherit {
                tab.instance = first.clone();
            }
        }
        reseat_tabs(
            &mut next,
            std::slice::from_ref(first),
            &old,
            &shape.profiles,
        );
    }
    next.instances.retain(|i| !remove.contains(&i.id));
    let at = position.min(next.instances.len());
    next.instances.splice(at..at, added);
    let used: Vec<DefinitionId> = next
        .instances
        .iter()
        .map(|i| i.definition.clone())
        .collect();
    next.definitions.retain(|id, _| used.contains(id));
    Ok(next)
}
