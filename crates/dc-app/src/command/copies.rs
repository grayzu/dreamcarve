// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/helpers/featureDefinitions.ts
//   src/store/helpers/copyFeatures.ts

//! The commands that make copies (functional spec, capability 98):
//! **Make Unique**, which gives a linked copy a definition of its own;
//! the linked copies an Alt-drag (and, later, an array) places; and
//! **Paste**, whose shapes are each a new instance of a new definition.
//!
//! Where it leaves PureCutCNC: a definition's id is the next free
//! `d<n>`, as a drawn shape's is, not `def-clone-<n>`; and a paste is
//! never linked, where upstream has a project-wide copy mode.

use dc_geometry::Transform;
use dc_project::{DefinitionId, FeatureInstance, InstanceId, Project};

use super::instances::{copy_name, find, next_free_id, require_all};
use crate::clipboard::Clip;
use crate::error::CommandError;

/// `MakeUnique`: each of `ids` that shares its definition with another
/// instance gets a copy of the definition under the next free id, and
/// lies where it lay (`makeUnique`). The ids are taken in order, so of
/// every copy of one shape the last keeps the definition they shared.
/// Refused when none is a linked copy.
pub(super) fn make_unique(project: &Project, ids: &[InstanceId]) -> Result<Project, CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    require_all(project, ids)?;
    let mut next = project.clone();
    let mut changed = false;
    for id in ids {
        let shared = find(&next, id)?.definition.clone();
        let linked = next
            .instances
            .iter()
            .any(|other| other.id != *id && other.definition == shared);
        if !linked {
            continue;
        }
        let taken: Vec<&str> = next.definitions.keys().map(DefinitionId::as_str).collect();
        let own = DefinitionId::new(next_free_id(&taken, "d"));
        let mut definition = next
            .definitions
            .get(&shared)
            .ok_or_else(|| CommandError::UnknownDefinition {
                instance: id.clone(),
                definition: shared.clone(),
            })?
            .clone();
        definition.id = own.clone();
        next.definitions.insert(own.clone(), definition);
        if let Some(instance) = next.instances.iter_mut().find(|i| i.id == *id) {
            instance.definition = own;
        }
        changed = true;
    }
    if changed {
        Ok(next)
    } else {
        Err(CommandError::NotLinked)
    }
}

/// `CopyInstances`: for each of `placements`, a linked copy of each of
/// `ids`, placed by the placement after the instance's own transform and
/// named as a duplicate is; with `folder`, they and their sources are
/// filed in a new group of that name. The copies of one placement are in the
/// Design Tree's order, whatever order the selection was made in
/// (`sourcesInAuthoredOrder`): the order shapes are listed in is the
/// order they are cut in.
pub(super) fn copy(
    project: &Project,
    ids: &[InstanceId],
    placements: &[Transform],
    folder: Option<&str>,
) -> Result<Project, CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    if placements.is_empty() {
        return Err(CommandError::NothingToDo);
    }
    require_all(project, ids)?;
    let sources: Vec<&FeatureInstance> = project
        .instances
        .iter()
        .filter(|instance| ids.contains(&instance.id))
        .collect();
    let mut next = project.clone();
    for placement in placements {
        for original in &sources {
            let transform = original.transform.then(*placement);
            // A project every reader would refuse is refused here.
            if let Some(definition) = project.definitions.get(&original.definition) {
                for profile in &definition.profiles {
                    transform.apply_polyline(profile)?;
                }
            }
            let taken_ids: Vec<&str> = next.instances.iter().map(|i| i.id.as_str()).collect();
            let taken_names: Vec<&str> = next.instances.iter().map(|i| i.name.as_str()).collect();
            let copy = FeatureInstance {
                id: InstanceId::new(next_free_id(&taken_ids, "i")),
                name: copy_name(&taken_names, &original.name),
                transform,
                locked: false,
                ..(*original).clone()
            };
            next.instances.push(copy);
        }
    }
    // An array's copies and their sources are one group.
    if let Some(name) = folder {
        let made: Vec<InstanceId> = next
            .instances
            .iter()
            .skip(project.instances.len())
            .map(|instance| instance.id.clone())
            .collect();
        let mut filed = ids.to_vec();
        filed.extend(made);
        next = super::folders::new_folder(&next, name, &filed, true)?;
    }
    Ok(next)
}

/// `PasteShapes`: each clip a new instance of a new definition, at the
/// clip's transform, visible and unlocked. A name the project already
/// has becomes a copy's (`square (2)`); a free one is kept. A clip's
/// folder is kept when this project has it.
pub(super) fn paste(project: &Project, clips: &[Clip]) -> Result<Project, CommandError> {
    if clips.is_empty() {
        return Err(CommandError::NothingToDo);
    }
    let mut next = project.clone();
    for clip in clips {
        if clip.definition.profiles.is_empty() {
            return Err(CommandError::EmptyOutline);
        }
        for profile in &clip.definition.profiles {
            clip.instance.transform.apply_polyline(profile)?;
        }
        let definitions: Vec<&str> = next.definitions.keys().map(DefinitionId::as_str).collect();
        let definition = DefinitionId::new(next_free_id(&definitions, "d"));
        let instances: Vec<&str> = next.instances.iter().map(|i| i.id.as_str()).collect();
        let instance = InstanceId::new(next_free_id(&instances, "i"));
        let names: Vec<&str> = next.instances.iter().map(|i| i.name.as_str()).collect();
        let name = if names.contains(&clip.instance.name.as_str()) {
            copy_name(&names, &clip.instance.name)
        } else {
            clip.instance.name.clone()
        };
        let folder = clip
            .instance
            .folder
            .clone()
            .filter(|folder| next.folders.iter().any(|f| f.id == *folder));
        let mut pasted = clip.definition.clone();
        pasted.id = definition.clone();
        pasted.name.clone_from(&name);
        next.definitions.insert(definition.clone(), pasted);
        next.instances.push(FeatureInstance {
            id: instance,
            definition,
            name,
            visible: true,
            locked: false,
            folder,
            ..clip.instance.clone()
        });
    }
    Ok(next)
}
