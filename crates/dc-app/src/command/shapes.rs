// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The commands that write a shape's geometry
//! (`docs/specs/phase-5/design.md`, "Data model → dc-app"): a drawn shape
//! added, as a new instance of a new definition (functional capability
//! 90); a definition's outline replaced, with the tabs on it re-seated;
//! a shape that keeps its sizes made a plain path (capability 84).

use dc_geometry::{Point, Polyline, Rect, Transform, nearest_point};
use dc_project::{
    DefinitionId, FeatureDefinition, FeatureInstance, GroupId, InstanceId, Project, Shape,
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
    /// A text's words (capability 91).
    Text,
    /// A text's font.
    Font,
    /// How a text's lines are set.
    Align,
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
    /// Shapes offset: copies a distance outside or inside their outlines
    /// (capability 94).
    Offset,
    /// Shapes welded into their union (capability 95).
    Weld,
    /// The other shapes taken out of the first.
    Subtract,
    /// What the shapes all share.
    Intersect,
    /// A piece of an outline removed between two crossings (capability
    /// 96).
    Trim,
    /// Open paths whose ends meet made one (capability 97).
    Join,
    /// Corners rounded by arcs (capability 93).
    Fillet {
        /// How many.
        count: usize,
    },
    /// Corners cut off straight.
    Chamfer {
        /// How many.
        count: usize,
    },
    /// A span made a line, an arc or a curve (capability 92, Segment
    /// kind).
    SegmentKind(dc_geometry::SpanKind),
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
        Edit::Offset => with(StrKey::CommandLabelOffset),
        Edit::Weld => with(StrKey::CommandLabelWeld),
        Edit::Subtract => with(StrKey::CommandLabelSubtract),
        Edit::Intersect => with(StrKey::CommandLabelIntersect),
        Edit::Trim => with(StrKey::CommandLabelTrim),
        Edit::Join => with(StrKey::CommandLabelJoinPaths),
        Edit::Fillet { count: 1 } => with(StrKey::CommandLabelFillet),
        Edit::Fillet { count } => counted(StrKey::CommandLabelFillets, count),
        Edit::Chamfer { count: 1 } => with(StrKey::CommandLabelChamfer),
        Edit::Chamfer { count } => counted(StrKey::CommandLabelChamfers, count),
        Edit::SegmentKind(dc_geometry::SpanKind::Line) => with(StrKey::CommandLabelMakeStraight),
        Edit::SegmentKind(dc_geometry::SpanKind::Arc) => with(StrKey::CommandLabelMakeArc),
        Edit::SegmentKind(dc_geometry::SpanKind::Cubic) => with(StrKey::CommandLabelMakeCurve),
        Edit::Text => with(StrKey::CommandLabelEditText),
        Edit::Size(_)
        | Edit::Corners
        | Edit::Measured
        | Edit::Font
        | Edit::Align
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
    /// The group the instance is filed in.
    pub group: Option<GroupId>,
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
/// arc under an uneven scale); [`CommandError::UnknownGroup`] for a
/// group the project does not have.
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
        if let Some(group) = &shape.group
            && !next.groups.iter().any(|f| &f.id == group)
        {
            return Err(CommandError::UnknownGroup(group.clone()));
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
            group: shape.group.clone(),
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

/// `ReplaceShapes`: the shapes `new`, made from the instances `from`,
/// each a new instance of a new definition, in the Design Tree where the
/// first of `from` is (after the last of them when they are kept).
///
/// Unless `keep`, the instances `from` go. For each pair of `inherit`, a
/// shape of `from` and an index into `new`: an operation that targeted
/// that shape, and an Inlay that had it in its boundary, has that new
/// shape in its place, and its tabs move to the new shape, re-seated on
/// its outline (it is placed as the old one was, so the tabs' points are
/// in the same space). The other removed shapes leave every target list
/// and their tabs go.
///
/// # Errors
///
/// [`CommandError::NothingSelected`] for no shapes to make them from,
/// [`CommandError::NothingToDo`] for nothing to add,
/// [`CommandError::Locked`] when a shape to remove is locked, and
/// [`add`]'s for a shape that cannot be.
pub(super) fn replace(
    project: &Project,
    from: &[InstanceId],
    keep: bool,
    new: &[NewShape],
    inherit: &[(InstanceId, usize)],
) -> Result<Project, CommandError> {
    if from.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    let mut locked = Vec::new();
    for id in from {
        let instance = find(project, id)?;
        if instance.locked && !keep {
            locked.push(instance.name.clone());
        }
    }
    if !locked.is_empty() {
        return Err(CommandError::Locked { names: locked });
    }
    let is_from = |i: &FeatureInstance| from.contains(&i.id);
    let mut next = add(project, new)?;
    let added: Vec<FeatureInstance> = next.instances.split_off(project.instances.len());
    if keep {
        // Beside what they were made from.
        let after = project
            .instances
            .iter()
            .rposition(is_from)
            .map_or(project.instances.len(), |at| at + 1);
        next.instances.splice(after..after, added);
        return Ok(next);
    }
    let position = project
        .instances
        .iter()
        .position(is_from)
        .unwrap_or(project.instances.len());
    let heir = |old: &InstanceId| -> Option<&FeatureInstance> {
        let (_, index) = inherit.iter().find(|(id, _)| id == old)?;
        added.get(*index)
    };
    // What cut or bounded an inherited shape cuts or bounds its heir.
    let retarget = |targets: &mut Vec<InstanceId>| {
        let mut kept: Vec<InstanceId> = Vec::with_capacity(targets.len());
        for target in targets.drain(..) {
            let next_target = if from.contains(&target) {
                heir(&target).map(|new| new.id.clone())
            } else {
                Some(target)
            };
            if let Some(target) = next_target
                && !kept.contains(&target)
            {
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
    // An inherited shape's tabs go with its outline; the others' go.
    next.tabs
        .retain(|tab| !from.contains(&tab.instance) || heir(&tab.instance).is_some());
    for (old, index) in inherit {
        let (Some(new_instance), Some(shape)) = (added.get(*index), new.get(*index)) else {
            continue;
        };
        let Some(old_profiles) = find(project, old)
            .ok()
            .and_then(|instance| project.definitions.get(&instance.definition))
            .map(|definition| definition.profiles.clone())
        else {
            continue;
        };
        // Only this shape's tabs are re-seated: the heir may have others'
        // already.
        let mut moved = Project {
            tabs: Vec::new(),
            ..next.clone()
        };
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut next.tabs)
            .into_iter()
            .partition(|tab| &tab.instance == old);
        moved.tabs = mine
            .into_iter()
            .map(|mut tab| {
                tab.instance = new_instance.id.clone();
                tab
            })
            .collect();
        reseat_tabs(
            &mut moved,
            std::slice::from_ref(&new_instance.id),
            &old_profiles,
            &shape.profiles,
        );
        next.tabs = rest;
        next.tabs.extend(moved.tabs);
    }
    next.instances.retain(|i| !from.contains(&i.id));
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
