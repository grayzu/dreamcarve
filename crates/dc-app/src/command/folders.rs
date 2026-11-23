// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/slices/featureSlice.ts

//! The folder commands (functional spec, capability 99): a folder made
//! round the selection, renamed, deleted with its shapes, dissolved, and
//! rows moved within the Design Tree, into a folder and out of one
//! (`addFeatureFolder`, `groupSelectedFeaturesIntoNewFolder`,
//! `moveFeatureTreeFeature`).
//!
//! Where it leaves PureCutCNC: there is no tree beside the shapes. A
//! folder's rows are the instances that name it, and every command
//! leaves them next to each other in the file, so the order listed is
//! the order drawn and cut ([`gather`]). Deleting a folder deletes its
//! shapes (upstream keeps them); *Ungroup* is what keeps them. A shape
//! can be dragged out of a group, and a group left with no shape goes.

use dc_project::{Folder, FolderId, InstanceId, Project};

use super::instances::{delete, next_free_id, require_all};
use crate::error::CommandError;

/// The folder `id`.
pub(super) fn find<'a>(project: &'a Project, id: &FolderId) -> Result<&'a Folder, CommandError> {
    project
        .folders
        .iter()
        .find(|folder| folder.id == *id)
        .ok_or_else(|| CommandError::UnknownFolder(id.clone()))
}

/// The instances filed in folder `id`, in file order.
pub(super) fn members(project: &Project, id: &FolderId) -> Vec<InstanceId> {
    project
        .instances
        .iter()
        .filter(|instance| instance.folder.as_ref() == Some(id))
        .map(|instance| instance.id.clone())
        .collect()
}

/// Puts each folder's rows next to each other, after the first of them,
/// in the order they have ([`crate::tree::gathered`]).
pub(super) fn gather(project: &mut Project) {
    let order = crate::tree::gathered(project);
    if order.iter().enumerate().all(|(at, index)| at == *index) {
        return;
    }
    let mut taken: Vec<Option<dc_project::FeatureInstance>> =
        std::mem::take(&mut project.instances)
            .into_iter()
            .map(Some)
            .collect();
    project.instances = order
        .into_iter()
        .filter_map(|index| taken.get_mut(index).and_then(Option::take))
        .collect();
}

/// Removes every group no instance is filed in: a group is its shapes.
pub(super) fn prune_groups(project: &mut Project) {
    let used: Vec<FolderId> = project
        .instances
        .iter()
        .filter_map(|instance| instance.folder.clone())
        .collect();
    project
        .folders
        .retain(|folder| !folder.group || used.contains(&folder.id));
}

/// What every command leaves of the folders: no empty group, and each
/// folder's rows together. Run when a command changed which instances
/// there are or where one is filed.
pub(super) fn settle(project: &Project, next: &mut Project) {
    if next.folders.is_empty() {
        return;
    }
    let layout = |p: &Project| -> Vec<(InstanceId, Option<FolderId>)> {
        p.instances
            .iter()
            .map(|instance| (instance.id.clone(), instance.folder.clone()))
            .collect()
    };
    if layout(project) != layout(next) {
        prune_groups(next);
        gather(next);
    }
}

/// `NewFolder`: a folder under the next free `f<n>`, holding `ids`,
/// which are gathered where the first of them is. A group needs two
/// shapes; a plain folder may be made empty.
pub(super) fn new_folder(
    project: &Project,
    name: &str,
    ids: &[InstanceId],
    group: bool,
) -> Result<Project, CommandError> {
    let name = name.trim();
    if name.is_empty() || (group && ids.len() < 2) {
        return Err(CommandError::NothingToDo);
    }
    if !ids.is_empty() {
        require_all(project, ids)?;
    }
    let mut next = project.clone();
    let taken: Vec<&str> = next.folders.iter().map(|f| f.id.as_str()).collect();
    let id = FolderId::new(next_free_id(&taken, "f"));
    next.folders.push(Folder {
        id: id.clone(),
        name: name.to_owned(),
        group,
    });
    for instance in &mut next.instances {
        if ids.contains(&instance.id) {
            instance.folder = Some(id.clone());
        }
    }
    prune_groups(&mut next);
    gather(&mut next);
    Ok(next)
}

/// `RenameFolder`.
pub(super) fn rename(
    project: &Project,
    id: &FolderId,
    name: &str,
) -> Result<Project, CommandError> {
    let name = name.trim();
    if name.is_empty() || find(project, id)?.name == name {
        return Err(CommandError::NothingToDo);
    }
    let mut next = project.clone();
    for folder in &mut next.folders {
        if folder.id == *id {
            name.clone_into(&mut folder.name);
        }
    }
    Ok(next)
}

/// `DeleteFolder`: the folder and its shapes, each as Delete removes it.
pub(super) fn delete_folder(project: &Project, id: &FolderId) -> Result<Project, CommandError> {
    find(project, id)?;
    let ids = members(project, id);
    let mut next = if ids.is_empty() {
        project.clone()
    } else {
        delete(project, &ids)?
    };
    next.folders.retain(|folder| folder.id != *id);
    Ok(next)
}

/// `Ungroup`: the folders go; their rows stay where they are, in no
/// folder.
pub(super) fn ungroup(project: &Project, ids: &[FolderId]) -> Result<Project, CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingToDo);
    }
    for id in ids {
        find(project, id)?;
    }
    let mut next = project.clone();
    for instance in &mut next.instances {
        if instance.folder.as_ref().is_some_and(|id| ids.contains(id)) {
            instance.folder = None;
        }
    }
    next.folders.retain(|folder| !ids.contains(&folder.id));
    Ok(next)
}

/// `MoveRows`: `ids`, in the order they have, are filed in `folder` and
/// placed before row `before`; with no `before`, after the folder's
/// last row, or at the end of the file when they are filed in none.
pub(super) fn move_rows(
    project: &Project,
    ids: &[InstanceId],
    before: Option<&InstanceId>,
    folder: Option<&FolderId>,
) -> Result<Project, CommandError> {
    require_all(project, ids)?;
    if let Some(folder) = folder {
        find(project, folder)?;
    }
    if let Some(before) = before {
        require_all(project, std::slice::from_ref(before))?;
    }
    let mut next = project.clone();
    let (mut moved, mut rest): (Vec<_>, Vec<_>) = std::mem::take(&mut next.instances)
        .into_iter()
        .partition(|instance| ids.contains(&instance.id));
    for instance in &mut moved {
        instance.folder = folder.cloned();
    }
    // A row cannot go before itself: before the next one that stays.
    let anchor = before.and_then(|before| {
        let from = project.instances.iter().position(|i| i.id == *before)?;
        project
            .instances
            .get(from..)?
            .iter()
            .find(|instance| !ids.contains(&instance.id))
            .map(|instance| instance.id.clone())
    });
    let at = match (&anchor, folder) {
        (Some(anchor), _) => rest.iter().position(|i| i.id == *anchor),
        (None, Some(folder)) if before.is_none() => rest
            .iter()
            .rposition(|i| i.folder.as_ref() == Some(folder))
            .map(|last| last.saturating_add(1)),
        _ => None,
    }
    .unwrap_or(rest.len());
    rest.splice(at..at, moved);
    next.instances = rest;
    prune_groups(&mut next);
    gather(&mut next);
    if next == *project {
        return Err(CommandError::NothingToDo);
    }
    Ok(next)
}
