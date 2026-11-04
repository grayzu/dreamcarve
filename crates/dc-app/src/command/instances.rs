// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The instance commands (functional spec, capabilities 58, 59 and 61):
//! transforms about the selection's bounding-box centre, linked copies,
//! deletion with its consequences, centring on the stock and fitting the
//! stock round the shapes. Bounds are the instances' outlines placed in
//! project space the way the preview places them: each definition profile
//! through the instance's transform.

use dc_geometry::{Point, Rect, Transform, approx_eq};
use dc_project::{FeatureInstance, InstanceId, Project};

use crate::error::CommandError;
use crate::strings::{StrKey, Strings};

/// How far a transform coefficient may be from 1 or 0 and still count as
/// it, when naming a transform for its label.
const COEFFICIENT_TOLERANCE: f64 = 1e-9;

/// How far a linked copy sits from its original, mm, in X and in Y
/// (capability 59).
const DUPLICATE_OFFSET_MM: f64 = 10.0;

/// The instance `id` of `project`.
pub(super) fn find<'a>(
    project: &'a Project,
    id: &InstanceId,
) -> Result<&'a FeatureInstance, CommandError> {
    project
        .instances
        .iter()
        .find(|i| i.id == *id)
        .ok_or_else(|| CommandError::UnknownInstance { id: id.clone() })
}

/// The instance `id` of `project`, to change.
pub(super) fn find_mut<'a>(
    project: &'a mut Project,
    id: &InstanceId,
) -> Result<&'a mut FeatureInstance, CommandError> {
    project
        .instances
        .iter_mut()
        .find(|i| i.id == *id)
        .ok_or_else(|| CommandError::UnknownInstance { id: id.clone() })
}

/// `ids` is not empty and every one exists.
fn require_all(project: &Project, ids: &[InstanceId]) -> Result<(), CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    for id in ids {
        find(project, id)?;
    }
    Ok(())
}

/// None of `ids` is locked (capability 58), else the locked ones' names.
fn require_unlocked(project: &Project, ids: &[InstanceId]) -> Result<(), CommandError> {
    let names: Vec<String> = ids
        .iter()
        .filter_map(|id| find(project, id).ok())
        .filter(|i| i.locked)
        .map(|i| i.name.clone())
        .collect();
    if names.is_empty() {
        Ok(())
    } else {
        Err(CommandError::Locked { names })
    }
}

/// The joint bounding box of `ids`' outlines in project space, mm.
///
/// # Errors
///
/// An unknown instance or definition, a transform that cannot place a
/// shape, or [`CommandError::NoShapes`] when no outline has a vertex.
pub(super) fn bounds(project: &Project, ids: &[InstanceId]) -> Result<Rect, CommandError> {
    let mut joint: Option<Rect> = None;
    for id in ids {
        let instance = find(project, id)?;
        let definition = project
            .definitions
            .get(&instance.definition)
            .ok_or_else(|| CommandError::UnknownDefinition {
                instance: id.clone(),
                definition: instance.definition.clone(),
            })?;
        for profile in &definition.profiles {
            let placed = instance.transform.apply_polyline(profile)?;
            if let Some(rect) = placed.bounds() {
                joint = Some(match joint {
                    Some(so_far) => so_far.including(rect.min).including(rect.max),
                    None => rect,
                });
            }
        }
    }
    joint.ok_or(CommandError::NoShapes)
}

/// The centre of `rect`.
fn centre(rect: Rect) -> Point {
    Point::new(
        (rect.min.x + rect.max.x) / 2.0,
        (rect.min.y + rect.max.y) / 2.0,
    )
}

/// `delta` as a change about `about`: move the centre to the origin,
/// apply, move it back.
fn about(about: Point, delta: Transform) -> Transform {
    Transform::translate(-about.x, -about.y)
        .then(delta)
        .then(Transform::translate(about.x, about.y))
}

/// `project` with `change` applied after each of `ids`' transforms.
fn shift_all(project: &Project, ids: &[InstanceId], change: Transform) -> Project {
    let mut next = project.clone();
    for instance in &mut next.instances {
        if ids.contains(&instance.id) {
            instance.transform = instance.transform.then(change);
        }
    }
    next
}

/// `TransformInstances`: `delta` about the selection's bounding-box centre
/// (capability 58).
pub(super) fn transform(
    project: &Project,
    ids: &[InstanceId],
    delta: Transform,
) -> Result<Project, CommandError> {
    require_all(project, ids)?;
    require_unlocked(project, ids)?;
    if !delta.coeffs().iter().all(|c| c.is_finite()) || delta.inverse().is_none() {
        return Err(CommandError::SingularTransform);
    }
    let pivot = centre(bounds(project, ids)?);
    Ok(shift_all(project, ids, about(pivot, delta)))
}

/// `CentreOnStock`: the selection's bounding box centred on the stock
/// (capability 61).
pub(super) fn centre_on_stock(
    project: &Project,
    ids: &[InstanceId],
) -> Result<Project, CommandError> {
    require_all(project, ids)?;
    require_unlocked(project, ids)?;
    let from = centre(bounds(project, ids)?);
    let to = Point::new(project.stock.width_mm / 2.0, project.stock.height_mm / 2.0);
    Ok(shift_all(
        project,
        ids,
        Transform::translate(to.x - from.x, to.y - from.y),
    ))
}

/// `FitStockToShapes`: the stock `init` would size round the visible
/// shapes with `margin_mm`, every instance shifted alike so the shapes
/// keep their places relative to each other (capability 61).
pub(super) fn fit_stock(project: &Project, margin_mm: f64) -> Result<Project, CommandError> {
    if !margin_mm.is_finite() || margin_mm < 0.0 {
        return Err(CommandError::InvalidValue {
            field: "margin_mm",
            value: margin_mm,
        });
    }
    let visible: Vec<InstanceId> = project
        .instances
        .iter()
        .filter(|i| i.visible)
        .map(|i| i.id.clone())
        .collect();
    if visible.is_empty() {
        return Err(CommandError::NoShapes);
    }
    let fit = dc_job::project_from::fit_stock(bounds(project, &visible)?, margin_mm);
    let all: Vec<InstanceId> = project.instances.iter().map(|i| i.id.clone()).collect();
    let mut next = shift_all(project, &all, fit.shift);
    next.stock.width_mm = fit.width_mm;
    next.stock.height_mm = fit.height_mm;
    Ok(next)
}

/// `DuplicateInstances`: a linked copy of each, sharing its definition,
/// offset by [`DUPLICATE_OFFSET_MM`], named `<name> (2)` or the next free
/// suffix, appended in selection order (capability 59).
pub(super) fn duplicate(project: &Project, ids: &[InstanceId]) -> Result<Project, CommandError> {
    require_all(project, ids)?;
    let mut next = project.clone();
    for id in ids {
        let original = find(project, id)?;
        let taken_ids: Vec<&str> = next.instances.iter().map(|i| i.id.as_str()).collect();
        let taken_names: Vec<&str> = next.instances.iter().map(|i| i.name.as_str()).collect();
        let copy = FeatureInstance {
            id: InstanceId::new(next_free_id(&taken_ids, "i")),
            name: copy_name(&taken_names, &original.name),
            transform: original.transform.then(Transform::translate(
                DUPLICATE_OFFSET_MM,
                DUPLICATE_OFFSET_MM,
            )),
            ..original.clone()
        };
        next.instances.push(copy);
    }
    Ok(next)
}

/// `DeleteInstances`: the instances go, with their placed tabs, their
/// entries in every operation's targets, and any definition no instance
/// uses after (capability 59). The operations stay.
pub(super) fn delete(project: &Project, ids: &[InstanceId]) -> Result<Project, CommandError> {
    require_all(project, ids)?;
    let mut next = project.clone();
    next.instances.retain(|i| !ids.contains(&i.id));
    next.tabs.retain(|t| !ids.contains(&t.instance));
    for operation in &mut next.operations {
        super::operations::targets_mut(operation).retain(|t| !ids.contains(t));
    }
    let used: Vec<_> = next
        .instances
        .iter()
        .map(|i| i.definition.clone())
        .collect();
    next.definitions.retain(|id, _| used.contains(id));
    Ok(next)
}

/// `<prefix><n>` for the smallest `n` past every numbered id in `taken`,
/// from 1.
pub(super) fn next_free_id(taken: &[&str], prefix: &str) -> String {
    let highest = taken
        .iter()
        .filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("{prefix}{}", highest.saturating_add(1))
}

/// `<name> (2)`, or the next suffix no name in `taken` has; a name that
/// already carries a suffix counts from its base, so the copy of
/// `Oval (2)` is `Oval (3)`.
pub(super) fn copy_name(taken: &[&str], name: &str) -> String {
    let base = strip_suffix(name);
    (2u32..)
        .map(|k| format!("{base} ({k})"))
        .find(|candidate| !taken.contains(&candidate.as_str()))
        .unwrap_or_else(|| format!("{base} (2)"))
}

/// `Oval (2)` → `Oval`; a name without a ` (n)` suffix as is.
fn strip_suffix(name: &str) -> &str {
    name.strip_suffix(')')
        .and_then(|s| s.rsplit_once(" ("))
        .filter(|(_, n)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        .map_or(name, |(base, _)| base)
}

/// What a transform does, for its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TransformKind {
    /// A translation.
    Move,
    /// A rotation (with or without a scale).
    Rotate,
    /// A scale along the axes.
    Scale,
    /// A reflection.
    Mirror,
    /// Anything else (a shear).
    Other,
}

/// Names `delta` by its linear part.
pub(super) fn classify(delta: Transform) -> TransformKind {
    let [a, b, c, d, _, _] = delta.coeffs();
    let is = |x: f64, y: f64| approx_eq(x, y, COEFFICIENT_TOLERANCE);
    if is(a, 1.0) && is(b, 0.0) && is(c, 0.0) && is(d, 1.0) {
        TransformKind::Move
    } else if a * d - b * c < 0.0 {
        TransformKind::Mirror
    } else if is(b, 0.0) && is(c, 0.0) {
        TransformKind::Scale
    } else if is(a, d) && is(b, -c) {
        TransformKind::Rotate
    } else {
        TransformKind::Other
    }
}

/// `“{name}”` for one instance, `{count} shapes` for several, as a label's
/// subject. An id the project does not have shows as the id.
pub(super) fn subject(strings: &Strings, project: &Project, ids: &[InstanceId]) -> String {
    match ids {
        [id] => {
            let name =
                find(project, id).map_or_else(|_| id.as_str().to_owned(), |i| i.name.clone());
            strings.fmt(StrKey::CommandSubjectOne, &[("name", &name)])
        }
        _ => strings.fmt(
            StrKey::CommandSubjectMany,
            &[("count", &ids.len().to_string())],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    #[test]
    fn copy_names_take_the_next_free_suffix() {
        assert_eq!(copy_name(&["Oval"], "Oval"), "Oval (2)");
        assert_eq!(copy_name(&["Oval", "Oval (2)"], "Oval"), "Oval (3)");
        assert_eq!(copy_name(&["Oval", "Oval (2)"], "Oval (2)"), "Oval (3)");
        assert_eq!(copy_name(&["x (a)"], "x (a)"), "x (a) (2)");
    }

    #[test]
    fn next_free_id_follows_the_highest() {
        assert_eq!(next_free_id(&["i1", "i7", "oval"], "i"), "i8");
        assert_eq!(next_free_id(&[], "i"), "i1");
    }

    #[test]
    fn transforms_are_named_by_their_linear_part() {
        assert_eq!(
            classify(Transform::translate(3.0, 4.0)),
            TransformKind::Move
        );
        assert_eq!(
            classify(Transform::rotate(FRAC_PI_2)),
            TransformKind::Rotate
        );
        assert_eq!(
            classify(Transform::rotate(0.3).then(Transform::scale(2.0, 2.0))),
            TransformKind::Rotate
        );
        assert_eq!(classify(Transform::scale(2.0, 0.5)), TransformKind::Scale);
        assert_eq!(classify(Transform::scale(-1.0, 1.0)), TransformKind::Mirror);
        assert_eq!(
            classify(Transform::mirror_x_about(5.0)),
            TransformKind::Mirror
        );
        assert_eq!(
            classify(Transform::from([1.0, 0.0, 0.5, 1.0, 0.0, 0.0])),
            TransformKind::Other
        );
    }

    #[test]
    fn a_change_about_a_point_leaves_that_point_fixed() {
        let pivot = Point::new(30.0, 20.0);
        let moved = about(pivot, Transform::rotate(FRAC_PI_2)).apply(pivot);
        assert!(approx_eq(moved.x, pivot.x, 1e-9) && approx_eq(moved.y, pivot.y, 1e-9));
    }
}
